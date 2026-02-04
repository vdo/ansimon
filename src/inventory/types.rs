use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Host {
    pub name: String,
    pub ansible_host: Option<String>,
    pub ansible_port: Option<u16>,
    pub ansible_user: Option<String>,
    pub ansible_ssh_private_key_file: Option<String>,
    pub groups: Vec<String>,
    pub vars: HashMap<String, String>,
    /// Keys set directly on the host definition (not inherited from groups).
    /// These take precedence and cannot be overwritten by group vars.
    host_level_vars: HashSet<String>,
}

impl Host {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ansible_host: None,
            ansible_port: None,
            ansible_user: None,
            ansible_ssh_private_key_file: None,
            groups: Vec::new(),
            vars: HashMap::new(),
            host_level_vars: HashSet::new(),
        }
    }

    pub fn effective_host(&self) -> &str {
        self.ansible_host.as_deref().unwrap_or(&self.name)
    }

    pub fn effective_port(&self) -> u16 {
        self.ansible_port.unwrap_or(22)
    }

    fn set_var(&mut self, key: &str, value: &str) {
        match key {
            "ansible_host" => self.ansible_host = Some(value.to_string()),
            "ansible_port" => {
                if let Ok(p) = value.parse() {
                    self.ansible_port = Some(p);
                }
            }
            "ansible_user" | "ansible_ssh_user" => self.ansible_user = Some(value.to_string()),
            "ansible_ssh_private_key_file" => {
                self.ansible_ssh_private_key_file = Some(value.to_string())
            }
            _ => {
                self.vars.insert(key.to_string(), value.to_string());
            }
        }
    }

    /// Apply a var from a host definition. Records it so group vars can't overwrite it.
    pub fn apply_host_var(&mut self, key: &str, value: &str) {
        self.set_var(key, value);
        self.host_level_vars.insert(key.to_string());
    }

    /// Apply a var from a group. Skips if the key was set at host level.
    pub fn apply_group_var(&mut self, key: &str, value: &str) {
        if !self.host_level_vars.contains(key) {
            self.set_var(key, value);
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Group {
    pub name: String,
    pub hosts: Vec<String>,
    pub children: Vec<String>,
    pub vars: HashMap<String, String>,
}

impl Group {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            hosts: Vec::new(),
            children: Vec::new(),
            vars: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub hosts: HashMap<String, Host>,
    pub groups: HashMap<String, Group>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all_hosts(&self) -> Vec<&Host> {
        let mut hosts: Vec<&Host> = self.hosts.values().collect();
        hosts.sort_by(|a, b| a.name.cmp(&b.name));
        hosts
    }

    pub fn hosts_in_group(&self, group_name: &str) -> Vec<String> {
        let mut result = Vec::new();
        if let Some(group) = self.groups.get(group_name) {
            result.extend(group.hosts.clone());
            for child in &group.children {
                result.extend(self.hosts_in_group(child));
            }
        }
        result
    }

    #[allow(dead_code)]
    pub fn group_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.groups.keys().cloned().collect();
        names.sort();
        names
    }
}
