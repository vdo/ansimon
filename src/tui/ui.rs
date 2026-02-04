use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;

use super::app::{App, SortColumn};
use crate::metrics::{HostStatus, Severity};

/// 8 column headers in display order.
const COLUMN_HEADERS: &[(&str, SortColumn)] = &[
    ("St", SortColumn::Status),
    ("Host", SortColumn::Name),
    ("Group", SortColumn::Group),
    ("CPU", SortColumn::Cpu),
    ("Mem", SortColumn::Memory),
    ("Disk", SortColumn::Disk),
    ("IOw", SortColumn::IoWait),
    ("Swap", SortColumn::Swap),
];

/// Column width constraints matching COLUMN_HEADERS order.
const COLUMN_WIDTHS: &[Constraint] = &[
    Constraint::Length(4),   // St
    Constraint::Min(15),     // Host
    Constraint::Length(12),  // Group
    Constraint::Length(10),  // CPU
    Constraint::Length(14),  // Mem
    Constraint::Length(10),  // Disk
    Constraint::Length(6),   // IOw
    Constraint::Length(12),  // Swap
];

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),   // Table
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);

    if app.show_detail {
        let table_detail = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);
        draw_table(f, app, table_detail[0]);
        draw_detail(f, app, table_detail[1]);
    } else {
        draw_table(f, app, chunks[1]);
    }

    draw_footer(f, app, chunks[2]);

    if app.show_help {
        draw_help_overlay(f);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let elapsed = app
        .last_poll
        .map(|t| {
            let secs = t.elapsed().as_secs();
            format!("{secs}s ago")
        })
        .unwrap_or_else(|| "never".to_string());

    let title = Line::from(vec![
        Span::styled(
            " Ansimon v0.1.0 ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("Hosts: {}/{} up", app.hosts_up(), app.hosts_total()),
            if app.hosts_up() == app.hosts_total() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            },
        ),
        Span::raw(" │ "),
        Span::styled(format!("Last poll: {elapsed}"), Style::default().fg(Color::DarkGray)),
        Span::raw(" │ "),
        Span::styled(
            format!("Sort: {} {}", app.sort_column.label(), if app.sort_ascending { "▲" } else { "▼" }),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" │ "),
        Span::styled("[?] Help", Style::default().fg(Color::DarkGray)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let header = Paragraph::new(title).block(block);
    f.render_widget(header, area);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_cells = COLUMN_HEADERS.iter().map(|(label, col)| {
        let style = if *col == app.sort_column {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let indicator = if *col == app.sort_column {
            if app.sort_ascending { " ▲" } else { " ▼" }
        } else {
            ""
        };
        Cell::from(format!("{label}{indicator}")).style(style)
    });

    let header = Row::new(header_cells)
        .style(Style::default().add_modifier(Modifier::BOLD))
        .height(1);

    let warn = app.warning_threshold;
    let crit = app.critical_threshold;

    let rows: Vec<Row> = app
        .visible_hosts
        .iter()
        .map(|host_name| {
            let hm = app.host_metrics.get(host_name);
            let host = app.hosts.iter().find(|h| h.name == *host_name);

            let status = hm
                .map(|m| m.status)
                .unwrap_or(HostStatus::Unknown);
            let status_indicator = status.indicator();
            let status_color = match status {
                HostStatus::Up => Color::Green,
                HostStatus::Down => Color::Red,
                HostStatus::Connecting => Color::Yellow,
                HostStatus::Unknown => Color::DarkGray,
            };

            let group = host
                .and_then(|h| h.groups.first())
                .cloned()
                .unwrap_or_default();

            let severity_color = |sev: &Severity| match sev {
                Severity::Ok => Color::Green,
                Severity::Warning => Color::Yellow,
                Severity::Critical => Color::Red,
            };

            let row_style = match hm.map(|m| m.status) {
                Some(HostStatus::Down) => Style::default().fg(Color::DarkGray),
                Some(HostStatus::Connecting) => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };

            match hm.and_then(|m| m.metrics.as_ref()) {
                Some(m) => {
                    let cpu_sev = m.cpu_severity(warn, crit);
                    let mem_sev = m.mem_severity(warn, crit);
                    let disk_sev = m.disk_severity(warn, crit);
                    let iow_sev = m.iowait_severity();

                    // Swap: N/A in white when not present, severity color otherwise
                    let swap_cell = if m.has_swap() {
                        let swap_sev = m.swap_severity();
                        Cell::from(m.swap_display()).style(Style::default().fg(severity_color(&swap_sev)))
                    } else {
                        Cell::from("N/A").style(Style::default().fg(Color::White))
                    };

                    Row::new(vec![
                        Cell::from(status_indicator.to_string()).style(Style::default().fg(status_color)),
                        Cell::from(host_name.clone()),
                        Cell::from(group),
                        Cell::from(m.cpu_display(warn, crit)).style(Style::default().fg(severity_color(&cpu_sev))),
                        Cell::from(m.mem_display(warn, crit)).style(Style::default().fg(severity_color(&mem_sev))),
                        Cell::from(m.disk_display(warn, crit)).style(Style::default().fg(severity_color(&disk_sev))),
                        Cell::from(m.iowait_display()).style(Style::default().fg(severity_color(&iow_sev))),
                        swap_cell,
                    ])
                    .style(row_style)
                }
                None => {
                    let placeholder = match hm.map(|m| m.status) {
                        Some(HostStatus::Connecting) => "...",
                        _ => "--",
                    };
                    let p = placeholder.to_string();
                    Row::new(vec![
                        Cell::from(status_indicator.to_string()).style(Style::default().fg(status_color)),
                        Cell::from(host_name.clone()),
                        Cell::from(group),
                        Cell::from(p.clone()),
                        Cell::from(p.clone()),
                        Cell::from(p.clone()),
                        Cell::from(p.clone()),
                        Cell::from(p),
                    ])
                    .style(row_style)
                }
            }
        })
        .collect();

    let table = Table::new(rows, COLUMN_WIDTHS.to_vec())
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Hosts "),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let warn = app.warning_threshold;
    let crit = app.critical_threshold;

    let content = if let Some(host_name) = app.selected_host() {
        let host = app.hosts.iter().find(|h| h.name == host_name);
        let hm = app.host_metrics.get(host_name);

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Host: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(host_name),
            ]),
        ];

        if let Some(host) = host {
            lines.push(Line::from(vec![
                Span::styled("Address: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(host.effective_host()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Port: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(host.effective_port().to_string()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Groups: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(host.groups.join(", ")),
            ]));
            if let Some(user) = &host.ansible_user {
                lines.push(Line::from(vec![
                    Span::styled("User: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(user.clone()),
                ]));
            }
        }

        lines.push(Line::from(""));

        if let Some(hm) = hm {
            let status_color = match hm.status {
                HostStatus::Up => Color::Green,
                HostStatus::Down => Color::Red,
                HostStatus::Connecting => Color::Yellow,
                HostStatus::Unknown => Color::DarkGray,
            };
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{} {:?}", hm.status.indicator(), hm.status),
                    Style::default().fg(status_color),
                ),
            ]));

            if let Some(ref err) = hm.error {
                lines.push(Line::from(vec![
                    Span::styled("Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled(err.clone(), Style::default().fg(Color::Red)),
                ]));
            }

            if let Some(ref m) = hm.metrics {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("-- Metrics --", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("CPU:      ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.cpu_display(warn, crit)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Memory:   ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.mem_display(warn, crit)),
                    Span::raw(format!(" ({:.0}%)", m.mem_percent())),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Disk:     ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.disk_display(warn, crit)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("IO Wait:  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.iowait_display()),
                ]));
                if m.has_swap() {
                    lines.push(Line::from(vec![
                        Span::styled("Swap:     ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(m.swap_display()),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("Swap:     ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled("N/A", Style::default().fg(Color::White)),
                    ]));
                }
                lines.push(Line::from(vec![
                    Span::styled("Load:     ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{:.2} / {:.2} / {:.2}", m.load_1, m.load_5, m.load_15)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Net I/O:  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("RX {} / TX {}",
                        crate::metrics::human_bytes(m.net_rx_bytes_sec),
                        crate::metrics::human_bytes(m.net_tx_bytes_sec))),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("TCP:      ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.tcp_display()),
                    Span::raw(" connections"),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Procs:    ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{} running / {} total", m.procs_running, m.procs_total)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Disk I/O: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("R {} / W {}",
                        crate::metrics::human_bytes(m.disk_read_bytes_sec),
                        crate::metrics::human_bytes(m.disk_write_bytes_sec))),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("CPUs:     ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(m.num_cpus.to_string()),
                ]));

                let days = m.uptime_secs / 86400;
                let hours = (m.uptime_secs % 86400) / 3600;
                let mins = (m.uptime_secs % 3600) / 60;
                lines.push(Line::from(vec![
                    Span::styled("Uptime:   ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{}d {}h {}m", days, hours, mins)),
                ]));
            }

            if let Some(latency) = hm.ssh_latency_ms {
                lines.push(Line::from(vec![
                    Span::styled("SSH Lat:  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{}ms", latency)),
                ]));
            }

            if let Some(updated) = hm.last_updated {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Updated: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}s ago", updated.elapsed().as_secs()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        lines
    } else {
        vec![Line::from("No host selected")]
    };

    let detail = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Details "),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(detail, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let footer = if app.filter_mode {
        Line::from(vec![
            Span::styled(" /", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(&app.filter_text),
            Span::styled("█", Style::default().fg(Color::Cyan)),
            Span::styled("  (Enter confirm, Esc cancel)", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" q", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Quit  "),
            Span::styled("j/k", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Navigate  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Detail  "),
            Span::styled("s/S", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Sort  "),
            Span::styled("/", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Filter  "),
            Span::styled("r", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Refresh  "),
            Span::styled("?", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(":Help"),
            if !app.filter_text.is_empty() {
                Span::styled(
                    format!("  [filter: {}]", app.filter_text),
                    Style::default().fg(Color::Yellow),
                )
            } else {
                Span::raw("")
            },
        ])
    };

    let footer_widget = Paragraph::new(footer)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(footer_widget, area);
}

fn draw_help_overlay(f: &mut Frame) {
    let area = centered_rect(50, 60, f.area());
    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(Span::styled(
            "Ansimon Help",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  q / Ctrl-C  ", Style::default().fg(Color::Yellow)),
            Span::raw("Quit"),
        ]),
        Line::from(vec![
            Span::styled("  j/k / ↑/↓   ", Style::default().fg(Color::Yellow)),
            Span::raw("Navigate up/down"),
        ]),
        Line::from(vec![
            Span::styled("  g / G       ", Style::default().fg(Color::Yellow)),
            Span::raw("Go to first/last"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl-D/U    ", Style::default().fg(Color::Yellow)),
            Span::raw("Page down/up"),
        ]),
        Line::from(vec![
            Span::styled("  Enter       ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle detail panel"),
        ]),
        Line::from(vec![
            Span::styled("  s / S       ", Style::default().fg(Color::Yellow)),
            Span::raw("Cycle sort / Reverse sort"),
        ]),
        Line::from(vec![
            Span::styled("  /           ", Style::default().fg(Color::Yellow)),
            Span::raw("Filter hosts by name/group"),
        ]),
        Line::from(vec![
            Span::styled("  r           ", Style::default().fg(Color::Yellow)),
            Span::raw("Force refresh all hosts"),
        ]),
        Line::from(vec![
            Span::styled("  ?           ", Style::default().fg(Color::Yellow)),
            Span::raw("Toggle this help"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Help "),
    );

    f.render_widget(help, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
