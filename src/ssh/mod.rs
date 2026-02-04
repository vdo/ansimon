pub mod commands;

use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::sync::{mpsc, Semaphore};

use crate::cli::ResolvedArgs;
use crate::inventory::types::Host;
use crate::metrics::{HostMetrics, HostStatus};

/// Message sent from SSH polling tasks back to the TUI.
#[derive(Debug)]
pub enum SshMessage {
    /// Host is being polled
    Connecting(String),
    /// Poll result for a host
    Result(HostMetrics),
}

/// Spawn the SSH polling loop. Returns a receiver for results.
pub fn spawn_poller(
    hosts: Vec<Host>,
    args: Arc<ResolvedArgs>,
    interval_secs: u64,
) -> mpsc::UnboundedReceiver<SshMessage> {
    let (tx, rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let semaphore = Arc::new(Semaphore::new(args.forks));

        loop {
            let mut handles = Vec::new();

            for host in &hosts {
                let host = host.clone();
                let args = args.clone();
                let tx = tx.clone();
                let sem = semaphore.clone();

                let handle = tokio::spawn(async move {
                    let _permit = sem.acquire().await.ok();

                    let _ = tx.send(SshMessage::Connecting(host.name.clone()));

                    let result = poll_host(&host, &args).await;
                    let _ = tx.send(SshMessage::Result(result));
                });

                handles.push(handle);
            }

            // Wait for all polls to complete
            for handle in handles {
                let _ = handle.await;
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;
        }
    });

    rx
}

async fn poll_host(host: &Host, args: &ResolvedArgs) -> HostMetrics {
    let mut metrics = HostMetrics::new(&host.name);

    let effective_host = host.effective_host();
    let effective_port = args.port.unwrap_or_else(|| host.effective_port());
    let effective_user = args
        .user
        .as_deref()
        .or(host.ansible_user.as_deref());
    let effective_key = args
        .key
        .as_deref()
        .or(host.ansible_ssh_private_key_file.as_deref());

    let mut cmd = Command::new("ssh");

    // SSH options for non-interactive, batch mode
    cmd.arg("-o").arg("BatchMode=yes")
        .arg("-o").arg(format!("ConnectTimeout={}", args.ssh_timeout))
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg("-o").arg("LogLevel=ERROR");

    cmd.arg("-p").arg(effective_port.to_string());

    if let Some(key) = effective_key {
        cmd.arg("-i").arg(key);
    }

    let target = if let Some(user) = effective_user {
        format!("{user}@{effective_host}")
    } else {
        effective_host.to_string()
    };

    cmd.arg(&target);
    cmd.arg(commands::metrics_command());

    // Measure SSH latency (includes the remote sleep 1)
    let start = Instant::now();

    match cmd.output().await {
        Ok(output) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            // Subtract the 1000ms remote sleep to get actual SSH + parse latency
            let ssh_latency = elapsed_ms.saturating_sub(1000);

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match commands::parse_metrics_output(&stdout) {
                    Ok(m) => {
                        metrics.status = HostStatus::Up;
                        metrics.metrics = Some(m);
                        metrics.last_updated = Some(Instant::now());
                        metrics.ssh_latency_ms = Some(ssh_latency);
                    }
                    Err(e) => {
                        metrics.status = HostStatus::Down;
                        metrics.error = Some(format!("Parse error: {e}"));
                        metrics.last_updated = Some(Instant::now());
                        metrics.ssh_latency_ms = Some(ssh_latency);
                    }
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                metrics.status = HostStatus::Down;
                metrics.error = Some(stderr.trim().to_string());
                metrics.last_updated = Some(Instant::now());
            }
        }
        Err(e) => {
            metrics.status = HostStatus::Down;
            metrics.error = Some(format!("SSH failed: {e}"));
            metrics.last_updated = Some(Instant::now());
        }
    }

    metrics
}
