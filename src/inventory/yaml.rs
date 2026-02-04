use anyhow::{Context, Result};
use serde_yaml::Value;

use super::types::{Group, Host, Inventory};

pub fn parse_yaml(content: &str) -> Result<Inventory> {
    let root: Value = serde_yaml::from_str(content).context("Failed to parse YAML inventory")?;

    let mut inventory = Inventory::new();
    inventory
        .groups
        .insert("all".to_string(), Group::new("all"));
    inventory
        .groups
        .insert("ungrouped".to_string(), Group::new("ungrouped"));

    if let Value::Mapping(root_map) = &root {
        // Handle top-level "all" group or treat entire doc as group definitions
        if let Some(all_value) = root_map.get(&Value::String("all".to_string())) {
            parse_group_value(&mut inventory, "all", all_value)?;
        } else {
            // Each top-level key is a group
            for (key, value) in root_map {
                if let Value::String(group_name) = key {
                    if !inventory.groups.contains_key(group_name) {
                        inventory
                            .groups
                            .insert(group_name.clone(), Group::new(group_name));
                    }
                    parse_group_value(&mut inventory, group_name, value)?;
                }
            }
        }
    }

    Ok(inventory)
}

fn parse_group_value(inventory: &mut Inventory, group_name: &str, value: &Value) -> Result<()> {
    if let Value::Mapping(map) = value {
        // 1. Process children FIRST so descendant hosts exist before vars are applied
        if let Some(children_value) = map.get(&Value::String("children".to_string())) {
            if let Value::Mapping(children_map) = children_value {
                for (child_key, child_value) in children_map {
                    if let Value::String(child_name) = child_key {
                        if !inventory.groups.contains_key(child_name) {
                            inventory
                                .groups
                                .insert(child_name.clone(), Group::new(child_name));
                        }

                        if let Some(group) = inventory.groups.get_mut(group_name) {
                            if !group.children.contains(child_name) {
                                group.children.push(child_name.clone());
                            }
                        }

                        parse_group_value(inventory, child_name, child_value)?;
                    }
                }
            }
        }

        // 2. Process direct hosts
        if let Some(hosts_value) = map.get(&Value::String("hosts".to_string())) {
            if let Value::Mapping(hosts_map) = hosts_value {
                for (host_key, host_vars) in hosts_map {
                    if let Value::String(host_name) = host_key {
                        let host = inventory
                            .hosts
                            .entry(host_name.clone())
                            .or_insert_with(|| Host::new(host_name));

                        if !host.groups.contains(&group_name.to_string()) {
                            host.groups.push(group_name.to_string());
                        }

                        // Parse host variables — these are recorded as host-level
                        // so group vars can never overwrite them
                        if let Value::Mapping(vars_map) = host_vars {
                            for (var_key, var_val) in vars_map {
                                if let Value::String(k) = var_key {
                                    let v = value_to_string(var_val);
                                    host.apply_host_var(k, &v);
                                }
                            }
                        }

                        // Add to group
                        if let Some(group) = inventory.groups.get_mut(group_name) {
                            if !group.hosts.contains(host_name) {
                                group.hosts.push(host_name.clone());
                            }
                        }

                        // Add to "all"
                        if group_name != "all" {
                            if let Some(all) = inventory.groups.get_mut("all") {
                                if !all.hosts.contains(host_name) {
                                    all.hosts.push(host_name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Process vars LAST - apply to direct hosts AND all descendant hosts
        //    Host-level vars are protected and won't be overwritten
        if let Some(vars_value) = map.get(&Value::String("vars".to_string())) {
            if let Value::Mapping(vars_map) = vars_value {
                // Collect all hosts: direct + via children (recursively)
                let host_names = inventory.hosts_in_group(group_name);

                for (var_key, var_val) in vars_map {
                    if let Value::String(k) = var_key {
                        let v = value_to_string(var_val);

                        if let Some(group) = inventory.groups.get_mut(group_name) {
                            group.vars.insert(k.clone(), v.clone());
                        }

                        for host_name in &host_names {
                            if let Some(host) = inventory.hosts.get_mut(host_name) {
                                host.apply_group_var(k, &v);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => format!("{v:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_inventory() {
        let content = r#"
all:
  children:
    web:
      hosts:
        web01:
          ansible_host: 192.168.1.1
        web02:
          ansible_host: 192.168.1.2
      vars:
        ansible_user: deploy
    db:
      hosts:
        db01:
          ansible_host: 192.168.1.10
          ansible_port: 2222
"#;
        let inv = parse_yaml(content).unwrap();
        assert_eq!(inv.hosts.len(), 3);
        assert_eq!(
            inv.hosts["web01"].ansible_host.as_deref(),
            Some("192.168.1.1")
        );
        assert_eq!(inv.hosts["db01"].ansible_port, Some(2222));
        assert_eq!(
            inv.hosts["web01"].ansible_user.as_deref(),
            Some("deploy")
        );
    }

    #[test]
    fn test_host_vars_override_group_vars() {
        let content = r#"
all:
  children:
    servers:
      vars:
        ansible_ssh_user: ubuntu
      hosts:
        server01:
          ansible_ssh_user: root
        server02:
"#;
        let inv = parse_yaml(content).unwrap();
        // Host-level var should win over group var
        assert_eq!(inv.hosts["server01"].ansible_user.as_deref(), Some("root"));
        // Host without override gets group var
        assert_eq!(inv.hosts["server02"].ansible_user.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn test_host_vars_survive_shared_group_name() {
        // When a group name (e.g. "rpcs") appears under two parents with
        // different vars, host-level vars must not be overwritten.
        let content = r#"
all:
  children:
    provider_a:
      vars:
        ansible_ssh_user: root
      children:
        prod:
          children:
            rpcs:
              hosts:
                rpc01:
                  ansible_ssh_user: root
                rpc02:
    provider_b:
      vars:
        ansible_ssh_user: ubuntu
      children:
        rpcs:
          hosts:
            rpc03:
"#;
        let inv = parse_yaml(content).unwrap();
        // Host-level var must survive the shared group
        assert_eq!(
            inv.hosts["rpc01"].ansible_user.as_deref(),
            Some("root"),
            "host-level var should not be overwritten by provider_b group var"
        );
        // Host without host-level override in shared group: last parent wins
        assert_eq!(
            inv.hosts["rpc02"].ansible_user.as_deref(),
            Some("ubuntu"),
        );
        // provider_b host gets ubuntu
        assert_eq!(
            inv.hosts["rpc03"].ansible_user.as_deref(),
            Some("ubuntu"),
        );
    }

    #[test]
    fn test_parent_vars_propagate_to_children() {
        let content = r#"
cloud:
  vars:
    region: us-east
    ansible_ssh_user: root
  children:
    nodes:
      vars:
        role: node
      hosts:
        node01:
          custom_label: "primary"
"#;
        let inv = parse_yaml(content).unwrap();
        assert_eq!(inv.hosts.len(), 1);
        let host = &inv.hosts["node01"];
        // ansible_ssh_user from parent group propagates down
        assert_eq!(host.ansible_user.as_deref(), Some("root"));
        // host is in the nodes group
        assert!(host.groups.contains(&"nodes".to_string()));
        // parent group var propagates
        assert_eq!(host.vars.get("region").map(|s| s.as_str()), Some("us-east"));
    }
}
