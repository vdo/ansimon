use std::collections::HashMap;
use std::time::Instant;

use ratatui::widgets::TableState;

use crate::inventory::types::Host;
use crate::metrics::{HostMetrics, HostStatus};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    Name,
    Group,
    Status,
    Cpu,
    Memory,
    Disk,
    IoWait,
    Swap,
}

impl SortColumn {
    pub fn next(self) -> Self {
        match self {
            SortColumn::Name => SortColumn::Group,
            SortColumn::Group => SortColumn::Status,
            SortColumn::Status => SortColumn::Cpu,
            SortColumn::Cpu => SortColumn::Memory,
            SortColumn::Memory => SortColumn::Disk,
            SortColumn::Disk => SortColumn::IoWait,
            SortColumn::IoWait => SortColumn::Swap,
            SortColumn::Swap => SortColumn::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortColumn::Name => "Host",
            SortColumn::Group => "Group",
            SortColumn::Status => "Status",
            SortColumn::Cpu => "CPU",
            SortColumn::Memory => "Memory",
            SortColumn::Disk => "Disk",
            SortColumn::IoWait => "IOw",
            SortColumn::Swap => "Swap",
        }
    }
}

pub struct App {
    pub hosts: Vec<Host>,
    pub host_metrics: HashMap<String, HostMetrics>,
    pub table_state: TableState,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub filter_text: String,
    pub filter_mode: bool,
    pub show_detail: bool,
    pub show_help: bool,
    pub last_poll: Option<Instant>,
    pub should_quit: bool,
    /// Sorted+filtered host names for current view
    pub visible_hosts: Vec<String>,
    /// Severity thresholds
    pub warning_threshold: f64,
    pub critical_threshold: f64,
}

impl App {
    pub fn new(hosts: Vec<Host>, warning_threshold: f64, critical_threshold: f64) -> Self {
        let host_names: Vec<String> = hosts.iter().map(|h| h.name.clone()).collect();
        let mut host_metrics = HashMap::new();
        for h in &hosts {
            host_metrics.insert(h.name.clone(), HostMetrics::new(&h.name));
        }

        let mut app = Self {
            hosts,
            host_metrics,
            table_state: TableState::default(),
            sort_column: SortColumn::Name,
            sort_ascending: true,
            filter_text: String::new(),
            filter_mode: false,
            show_detail: false,
            show_help: false,
            last_poll: None,
            should_quit: false,
            visible_hosts: host_names,
            warning_threshold,
            critical_threshold,
        };
        if !app.visible_hosts.is_empty() {
            app.table_state.select(Some(0));
        }
        app
    }

    pub fn set_connecting(&mut self, host_name: &str) {
        if let Some(m) = self.host_metrics.get_mut(host_name) {
            if m.status != HostStatus::Up {
                m.status = HostStatus::Connecting;
            }
        }
    }

    pub fn refresh_visible(&mut self) {
        let filter_lower = self.filter_text.to_lowercase();
        let mut visible: Vec<String> = self
            .hosts
            .iter()
            .filter(|h| {
                if filter_lower.is_empty() {
                    return true;
                }
                h.name.to_lowercase().contains(&filter_lower)
                    || h.groups.iter().any(|g| g.to_lowercase().contains(&filter_lower))
            })
            .map(|h| h.name.clone())
            .collect();

        let sort_col = self.sort_column;
        let ascending = self.sort_ascending;
        let metrics = &self.host_metrics;
        let hosts_map: HashMap<String, &Host> =
            self.hosts.iter().map(|h| (h.name.clone(), h)).collect();

        visible.sort_by(|a, b| {
            let cmp = match sort_col {
                SortColumn::Name => a.cmp(b),
                SortColumn::Group => {
                    let ga = hosts_map.get(a).map(|h| h.groups.first().cloned().unwrap_or_default()).unwrap_or_default();
                    let gb = hosts_map.get(b).map(|h| h.groups.first().cloned().unwrap_or_default()).unwrap_or_default();
                    ga.cmp(&gb)
                }
                SortColumn::Status => {
                    let sa = metrics.get(a).map(|m| m.status as u8).unwrap_or(0);
                    let sb = metrics.get(b).map(|m| m.status as u8).unwrap_or(0);
                    sa.cmp(&sb)
                }
                SortColumn::Cpu => {
                    let ca = metrics.get(a).and_then(|m| m.metrics.as_ref()).map(|m| m.cpu_percent).unwrap_or(-1.0);
                    let cb = metrics.get(b).and_then(|m| m.metrics.as_ref()).map(|m| m.cpu_percent).unwrap_or(-1.0);
                    ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::Memory => {
                    let ma = metrics.get(a).and_then(|m| m.metrics.as_ref()).map(|m| m.mem_percent()).unwrap_or(-1.0);
                    let mb = metrics.get(b).and_then(|m| m.metrics.as_ref()).map(|m| m.mem_percent()).unwrap_or(-1.0);
                    ma.partial_cmp(&mb).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::Disk => {
                    let da = metrics.get(a).and_then(|m| m.metrics.as_ref()).map(|m| m.disk_percent).unwrap_or(-1.0);
                    let db = metrics.get(b).and_then(|m| m.metrics.as_ref()).map(|m| m.disk_percent).unwrap_or(-1.0);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::IoWait => {
                    let ia = metrics.get(a).and_then(|m| m.metrics.as_ref()).map(|m| m.iowait_percent).unwrap_or(-1.0);
                    let ib = metrics.get(b).and_then(|m| m.metrics.as_ref()).map(|m| m.iowait_percent).unwrap_or(-1.0);
                    ia.partial_cmp(&ib).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::Swap => {
                    let sa = metrics.get(a).and_then(|m| m.metrics.as_ref()).map(|m| m.swap_used_gb).unwrap_or(-1.0);
                    let sb = metrics.get(b).and_then(|m| m.metrics.as_ref()).map(|m| m.swap_used_gb).unwrap_or(-1.0);
                    sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
                }
            };
            if ascending { cmp } else { cmp.reverse() }
        });

        self.visible_hosts = visible;

        // Fix selection
        let selected = self.table_state.selected().unwrap_or(0);
        if self.visible_hosts.is_empty() {
            self.table_state.select(None);
        } else if selected >= self.visible_hosts.len() {
            self.table_state.select(Some(self.visible_hosts.len() - 1));
        }
    }

    pub fn selected_host(&self) -> Option<&str> {
        let idx = self.table_state.selected()?;
        self.visible_hosts.get(idx).map(|s| s.as_str())
    }

    pub fn move_down(&mut self) {
        if self.visible_hosts.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        let next = if i >= self.visible_hosts.len() - 1 {
            self.visible_hosts.len() - 1
        } else {
            i + 1
        };
        self.table_state.select(Some(next));
    }

    pub fn move_up(&mut self) {
        if self.visible_hosts.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        let next = if i == 0 { 0 } else { i - 1 };
        self.table_state.select(Some(next));
    }

    pub fn page_down(&mut self, page_size: usize) {
        if self.visible_hosts.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        let next = (i + page_size).min(self.visible_hosts.len() - 1);
        self.table_state.select(Some(next));
    }

    pub fn page_up(&mut self, page_size: usize) {
        let i = self.table_state.selected().unwrap_or(0);
        let next = i.saturating_sub(page_size);
        self.table_state.select(Some(next));
    }

    pub fn go_home(&mut self) {
        if !self.visible_hosts.is_empty() {
            self.table_state.select(Some(0));
        }
    }

    pub fn go_end(&mut self) {
        if !self.visible_hosts.is_empty() {
            self.table_state.select(Some(self.visible_hosts.len() - 1));
        }
    }

    pub fn hosts_up(&self) -> usize {
        self.host_metrics
            .values()
            .filter(|m| m.status == HostStatus::Up)
            .count()
    }

    pub fn hosts_total(&self) -> usize {
        self.hosts.len()
    }
}
