use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub inventory: String,
    pub interval: u64,
    pub forks: usize,
    pub ssh_timeout: u64,
    pub user: Option<String>,
    pub key: Option<String>,
    pub port: Option<u16>,
    pub thresholds: Thresholds,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Thresholds {
    pub warning: f64,
    pub critical: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            inventory: "/etc/ansible/hosts".to_string(),
            interval: 10,
            forks: 10,
            ssh_timeout: 5,
            user: None,
            key: None,
            port: None,
            thresholds: Thresholds::default(),
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            warning: 60.0,
            critical: 85.0,
        }
    }
}

const DEFAULT_CONFIG_CONTENT: &str = r#"# Ansimon configuration
# CLI arguments override these values

# Default inventory file path
inventory: /etc/ansible/hosts

# Poll interval in seconds
interval: 10

# Maximum concurrent SSH connections
forks: 10

# SSH connection timeout in seconds
ssh_timeout: 5

# Severity thresholds (percentage)
thresholds:
  warning: 60
  critical: 85

# Default SSH user (uncomment to set)
# user: root

# Default SSH port (uncomment to set)
# port: 22

# Default SSH private key path (uncomment to set)
# key: ~/.ssh/id_rsa
"#;

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs_or_home().map(|p| p.join("config.yml"))
    }

    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            Self::create_default(&path);
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(contents) => match serde_yaml::from_str(&contents) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Warning: failed to parse config {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("Warning: failed to read config {}: {e}", path.display());
                Self::default()
            }
        }
    }

    fn create_default(path: &PathBuf) {
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("Warning: could not create config dir {}: {e}", parent.display());
                return;
            }
        }
        if let Err(e) = fs::write(path, DEFAULT_CONFIG_CONTENT) {
            eprintln!("Warning: could not write default config to {}: {e}", path.display());
        }
    }
}

fn dirs_or_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("ansimon"))
}
