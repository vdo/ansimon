use clap::Parser;

/// Ansimon - TUI monitor for Ansible inventories
#[derive(Parser, Debug, Clone)]
#[command(name = "ansimon", version, about)]
pub struct Args {
    /// Path to Ansible inventory file (INI or YAML)
    #[arg(short, long)]
    pub inventory: Option<String>,

    /// Limit to subset of hosts (supports glob patterns, groups, exclusion with !)
    #[arg(short, long)]
    pub limit: Option<String>,

    /// Poll interval in seconds
    #[arg(long)]
    pub interval: Option<u64>,

    /// SSH user (overrides inventory)
    #[arg(short, long)]
    pub user: Option<String>,

    /// Path to SSH private key
    #[arg(short, long)]
    pub key: Option<String>,

    /// SSH port (overrides inventory)
    #[arg(short, long)]
    pub port: Option<u16>,

    /// Maximum concurrent SSH connections
    #[arg(short, long)]
    pub forks: Option<usize>,
}

/// Resolved args after merging CLI + config + defaults
#[derive(Debug, Clone)]
pub struct ResolvedArgs {
    pub inventory: String,
    pub limit: Option<String>,
    pub interval: u64,
    pub user: Option<String>,
    pub key: Option<String>,
    pub port: Option<u16>,
    pub forks: usize,
    pub ssh_timeout: u64,
    pub warning_threshold: f64,
    pub critical_threshold: f64,
}
