use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    Unknown,
    Connecting,
    Up,
    Down,
}

impl HostStatus {
    pub fn indicator(&self) -> &'static str {
        match self {
            HostStatus::Unknown => "[--]",
            HostStatus::Connecting => "[..]",
            HostStatus::Up => "[UP]",
            HostStatus::Down => "[DN]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warning,
    Critical,
}

impl Severity {
    pub fn indicator(&self) -> &'static str {
        match self {
            Severity::Ok => "OK",
            Severity::Warning => "WR",
            Severity::Critical => "CR",
        }
    }

    pub fn from_percent(pct: f64, warning: f64, critical: f64) -> Self {
        if pct > critical {
            Severity::Critical
        } else if pct > warning {
            Severity::Warning
        } else {
            Severity::Ok
        }
    }
}

/// Format bytes/sec into human-readable form: "1.2K", "3.4M", "500B", etc.
pub fn human_bytes(n: u64) -> String {
    if n >= 1_073_741_824 {
        format!("{:.1}G", n as f64 / 1_073_741_824.0)
    } else if n >= 1_048_576 {
        format!("{:.1}M", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.1}K", n as f64 / 1024.0)
    } else {
        format!("{}B", n)
    }
}

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cpu_percent: f64,
    pub mem_used_gb: f64,
    pub mem_total_gb: f64,
    pub disk_percent: f64,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub uptime_secs: u64,
    pub num_cpus: u32,
    // New metrics
    pub iowait_percent: f64,
    pub swap_used_gb: f64,
    pub swap_total_gb: f64,
    pub net_rx_bytes_sec: u64,
    pub net_tx_bytes_sec: u64,
    pub tcp_conns: u32,
    pub procs_running: u32,
    pub procs_total: u32,
    pub disk_read_bytes_sec: u64,
    pub disk_write_bytes_sec: u64,
}

impl Metrics {
    pub fn cpu_severity(&self, warning: f64, critical: f64) -> Severity {
        Severity::from_percent(self.cpu_percent, warning, critical)
    }

    pub fn mem_severity(&self, warning: f64, critical: f64) -> Severity {
        if self.mem_total_gb > 0.0 {
            Severity::from_percent(self.mem_used_gb / self.mem_total_gb * 100.0, warning, critical)
        } else {
            Severity::Ok
        }
    }

    pub fn disk_severity(&self, warning: f64, critical: f64) -> Severity {
        Severity::from_percent(self.disk_percent, warning, critical)
    }

    pub fn mem_percent(&self) -> f64 {
        if self.mem_total_gb > 0.0 {
            self.mem_used_gb / self.mem_total_gb * 100.0
        } else {
            0.0
        }
    }

    pub fn swap_severity(&self) -> Severity {
        if self.swap_total_gb > 0.0 {
            let pct = self.swap_used_gb / self.swap_total_gb * 100.0;
            if pct > 80.0 {
                Severity::Critical
            } else if pct > 50.0 {
                Severity::Warning
            } else {
                Severity::Ok
            }
        } else {
            Severity::Ok
        }
    }

    pub fn iowait_severity(&self) -> Severity {
        if self.iowait_percent > 30.0 {
            Severity::Critical
        } else if self.iowait_percent > 10.0 {
            Severity::Warning
        } else {
            Severity::Ok
        }
    }

    pub fn cpu_display(&self, warning: f64, critical: f64) -> String {
        format!("{} {:.0}%", self.cpu_severity(warning, critical).indicator(), self.cpu_percent)
    }

    pub fn mem_display(&self, warning: f64, critical: f64) -> String {
        format!(
            "{} {:.1}/{:.0}G",
            self.mem_severity(warning, critical).indicator(),
            self.mem_used_gb,
            self.mem_total_gb
        )
    }

    pub fn disk_display(&self, warning: f64, critical: f64) -> String {
        format!("{} {:.0}%", self.disk_severity(warning, critical).indicator(), self.disk_percent)
    }

    pub fn iowait_display(&self) -> String {
        format!("{:.1}%", self.iowait_percent)
    }

    pub fn has_swap(&self) -> bool {
        self.swap_total_gb > 0.01
    }

    pub fn swap_display(&self) -> String {
        if !self.has_swap() {
            "N/A".to_string()
        } else {
            format!(
                "{} {:.1}/{:.0}G",
                self.swap_severity().indicator(),
                self.swap_used_gb,
                self.swap_total_gb
            )
        }
    }

    pub fn tcp_display(&self) -> String {
        format!("{}", self.tcp_conns)
    }
}

#[derive(Debug, Clone)]
pub struct HostMetrics {
    pub host_name: String,
    pub status: HostStatus,
    pub metrics: Option<Metrics>,
    pub last_updated: Option<Instant>,
    pub error: Option<String>,
    pub ssh_latency_ms: Option<u64>,
}

impl HostMetrics {
    pub fn new(host_name: &str) -> Self {
        Self {
            host_name: host_name.to_string(),
            status: HostStatus::Unknown,
            metrics: None,
            last_updated: None,
            error: None,
            ssh_latency_ms: None,
        }
    }
}
