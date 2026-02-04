use anyhow::{Context, Result};
use std::collections::HashMap;

use super::types::{Group, Host, Inventory};

#[derive(Debug, PartialEq)]
enum Section {
    None,
    Group(String),
    GroupVars(String),
    GroupChildren(String),
}

pub fn parse_ini(content: &str) -> Result<Inventory> {
    let mut inventory = Inventory::new();
    let mut section = Section::None;

    // Ensure "all" and "ungrouped" groups exist
    inventory
        .groups
        .insert("all".to_string(), Group::new("all"));
    inventory
        .groups
        .insert("ungrouped".to_string(), Group::new("ungrouped"));

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Section header
        if line.starts_with('[') && line.ends_with(']') {
            let header = &line[1..line.len() - 1];
            section = parse_section_header(header);

            // Ensure group exists
            match &section {
                Section::Group(name) | Section::GroupVars(name) | Section::GroupChildren(name) => {
                    if !inventory.groups.contains_key(name) {
                        inventory
                            .groups
                            .insert(name.clone(), Group::new(name));
                    }
                }
                Section::None => {}
            }
            continue;
        }

        match &section {
            Section::None | Section::Group(_) => {
                let group_name = match &section {
                    Section::Group(name) => name.clone(),
                    _ => "ungrouped".to_string(),
                };

                let (host_name, vars) = parse_host_line(line)
                    .with_context(|| format!("Failed to parse host line: {line}"))?;

                let host = inventory
                    .hosts
                    .entry(host_name.clone())
                    .or_insert_with(|| Host::new(&host_name));

                for (k, v) in &vars {
                    host.apply_host_var(k, v);
                }

                if !host.groups.contains(&group_name) {
                    host.groups.push(group_name.clone());
                }

                if let Some(group) = inventory.groups.get_mut(&group_name) {
                    if !group.hosts.contains(&host_name) {
                        group.hosts.push(host_name.clone());
                    }
                }

                // Also add to "all"
                if group_name != "all" {
                    if let Some(all) = inventory.groups.get_mut("all") {
                        if !all.hosts.contains(&host_name) {
                            all.hosts.push(host_name.clone());
                        }
                    }
                }
            }
            Section::GroupVars(group_name) => {
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();

                    if let Some(group) = inventory.groups.get_mut(group_name) {
                        group.vars.insert(key.to_string(), value.to_string());
                    }

                    // Apply group vars to existing hosts in this group
                    let host_names: Vec<String> = inventory
                        .groups
                        .get(group_name)
                        .map(|g| g.hosts.clone())
                        .unwrap_or_default();

                    for host_name in host_names {
                        if let Some(host) = inventory.hosts.get_mut(&host_name) {
                            host.apply_group_var(key, value);
                        }
                    }
                }
            }
            Section::GroupChildren(group_name) => {
                let child_name = line.trim().to_string();
                if let Some(group) = inventory.groups.get_mut(group_name) {
                    if !group.children.contains(&child_name) {
                        group.children.push(child_name.clone());
                    }
                }
                // Ensure child group exists
                if !inventory.groups.contains_key(&child_name) {
                    inventory
                        .groups
                        .insert(child_name.clone(), Group::new(&child_name));
                }
            }
        }
    }

    Ok(inventory)
}

fn parse_section_header(header: &str) -> Section {
    if let Some(name) = header.strip_suffix(":vars") {
        Section::GroupVars(name.to_string())
    } else if let Some(name) = header.strip_suffix(":children") {
        Section::GroupChildren(name.to_string())
    } else {
        Section::Group(header.to_string())
    }
}

fn parse_host_line(line: &str) -> Result<(String, HashMap<String, String>)> {
    let mut vars = HashMap::new();
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.is_empty() {
        anyhow::bail!("Empty host line");
    }

    let host_name = parts[0].to_string();

    for part in &parts[1..] {
        if let Some((key, value)) = part.split_once('=') {
            vars.insert(key.to_string(), value.to_string());
        }
    }

    Ok((host_name, vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ini() {
        let content = r#"
[web]
web01 ansible_host=192.168.1.1
web02 ansible_host=192.168.1.2

[db]
db01 ansible_host=192.168.1.10 ansible_port=2222

[web:vars]
ansible_user=deploy
"#;
        let inv = parse_ini(content).unwrap();
        assert_eq!(inv.hosts.len(), 3);
        assert!(inv.hosts.contains_key("web01"));
        assert_eq!(
            inv.hosts["db01"].ansible_host.as_deref(),
            Some("192.168.1.10")
        );
        assert_eq!(inv.hosts["db01"].ansible_port, Some(2222));
        assert_eq!(
            inv.hosts["web01"].ansible_user.as_deref(),
            Some("deploy")
        );
    }

    #[test]
    fn test_children() {
        let content = r#"
[web]
web01

[db]
db01

[prod:children]
web
db
"#;
        let inv = parse_ini(content).unwrap();
        let prod = &inv.groups["prod"];
        assert_eq!(prod.children.len(), 2);
        assert!(prod.children.contains(&"web".to_string()));
    }
}
