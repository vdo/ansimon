use super::types::Inventory;

/// Apply Ansible-style --limit pattern to filter hosts.
///
/// Supported patterns:
/// - `hostname` - exact match
/// - `web*` - glob pattern
/// - `groupname` - all hosts in group (checked first)
/// - `host1,host2` - union (comma-separated)
/// - `!pattern` - exclude hosts matching pattern
/// - `&pattern` - intersection (only hosts also matching pattern)
pub fn apply_limit(inventory: &Inventory, limit: &str) -> Vec<String> {
    let parts: Vec<&str> = limit.split(',').map(|s| s.trim()).collect();

    let mut included: Vec<String> = Vec::new();
    let mut excluded: Vec<String> = Vec::new();
    let mut intersections: Vec<Vec<String>> = Vec::new();

    for part in parts {
        if part.is_empty() {
            continue;
        }

        if let Some(pattern) = part.strip_prefix('!') {
            excluded.extend(resolve_pattern(inventory, pattern));
        } else if let Some(pattern) = part.strip_prefix('&') {
            intersections.push(resolve_pattern(inventory, pattern));
        } else {
            included.extend(resolve_pattern(inventory, part));
        }
    }

    // Remove duplicates from included
    included.sort();
    included.dedup();

    // Apply exclusions
    included.retain(|h| !excluded.contains(h));

    // Apply intersections
    for intersection in &intersections {
        included.retain(|h| intersection.contains(h));
    }

    included
}

fn resolve_pattern(inventory: &Inventory, pattern: &str) -> Vec<String> {
    // Check if pattern is a group name first
    if let Some(group) = inventory.groups.get(pattern) {
        let mut hosts = group.hosts.clone();
        // Include children recursively
        for child in &group.children {
            hosts.extend(inventory.hosts_in_group(child));
        }
        hosts.sort();
        hosts.dedup();
        return hosts;
    }

    // Check for exact host match
    if inventory.hosts.contains_key(pattern) {
        return vec![pattern.to_string()];
    }

    // Glob matching
    let all_hosts: Vec<String> = inventory.hosts.keys().cloned().collect();
    all_hosts
        .into_iter()
        .filter(|h| glob_match::glob_match(pattern, h))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::ini::parse_ini;

    fn test_inventory() -> Inventory {
        let content = r#"
[web]
web01
web02
web03

[db]
db01
db02

[cache]
cache01
"#;
        parse_ini(content).unwrap()
    }

    #[test]
    fn test_group_limit() {
        let inv = test_inventory();
        let result = apply_limit(&inv, "web");
        assert_eq!(result, vec!["web01", "web02", "web03"]);
    }

    #[test]
    fn test_glob_limit() {
        let inv = test_inventory();
        let result = apply_limit(&inv, "web*");
        assert!(result.contains(&"web01".to_string()));
        assert!(result.contains(&"web02".to_string()));
        assert!(!result.contains(&"db01".to_string()));
    }

    #[test]
    fn test_exclusion() {
        let inv = test_inventory();
        let result = apply_limit(&inv, "all,!db");
        assert!(result.contains(&"web01".to_string()));
        assert!(!result.contains(&"db01".to_string()));
    }

    #[test]
    fn test_exact_host() {
        let inv = test_inventory();
        let result = apply_limit(&inv, "web01");
        assert_eq!(result, vec!["web01"]);
    }
}
