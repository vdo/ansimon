mod cli;
mod config;
mod inventory;
mod metrics;
mod ssh;
mod tui;

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;

use cli::{Args, ResolvedArgs};
use config::Config;
use inventory::limit::apply_limit;

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = Args::parse();
    let config = Config::load();

    // Merge: CLI > config > defaults
    let args = ResolvedArgs {
        inventory: cli_args
            .inventory
            .or(Some(config.inventory))
            .unwrap_or_else(|| "/etc/ansible/hosts".to_string()),
        limit: cli_args.limit,
        interval: cli_args.interval.unwrap_or(config.interval),
        user: cli_args.user.or(config.user),
        key: cli_args.key.or(config.key),
        port: cli_args.port.or(config.port),
        forks: cli_args.forks.unwrap_or(config.forks),
        ssh_timeout: config.ssh_timeout,
        warning_threshold: config.thresholds.warning,
        critical_threshold: config.thresholds.critical,
    };

    // Load inventory
    let inv = inventory::load_inventory(&args.inventory)
        .with_context(|| format!("Failed to load inventory from: {}", args.inventory))?;

    // Get hosts, apply --limit if specified
    let hosts: Vec<inventory::types::Host> = if let Some(ref limit) = args.limit {
        let host_names = apply_limit(&inv, limit);
        if host_names.is_empty() {
            anyhow::bail!("No hosts matched the limit pattern: {limit}");
        }
        host_names
            .iter()
            .filter_map(|name| inv.hosts.get(name).cloned())
            .collect()
    } else {
        inv.all_hosts().into_iter().cloned().collect()
    };

    if hosts.is_empty() {
        anyhow::bail!("No hosts found in inventory: {}", args.inventory);
    }

    let num_hosts = hosts.len();
    eprintln!("Ansimon starting with {num_hosts} host(s)...");

    let args = Arc::new(args);
    tui::run(hosts, args).await
}
