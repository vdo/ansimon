pub mod ini;
pub mod limit;
pub mod types;
pub mod yaml;

use anyhow::{Context, Result};
use std::path::Path;
use types::Inventory;

/// Load an Ansible inventory file, auto-detecting format (INI vs YAML).
pub fn load_inventory(path: &str) -> Result<Inventory> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read inventory: {path}"))?;

    let content = content.trim();

    if is_yaml(path, content) {
        yaml::parse_yaml(content).context("Failed to parse YAML inventory")
    } else {
        ini::parse_ini(content).context("Failed to parse INI inventory")
    }
}

fn is_yaml(path: &str, content: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "yml" | "yaml" => true,
        "ini" | "cfg" => false,
        _ => {
            // Heuristic: if it starts with "---" or "all:" or contains top-level YAML mapping
            content.starts_with("---")
                || content.starts_with("all:")
                || content.starts_with("all:\n")
        }
    }
}
