pub mod app;
pub mod event;
pub mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event as ct_event;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::cli::ResolvedArgs;
use crate::inventory::types::Host;
use crate::ssh::{self, SshMessage};

use app::App;
use event::{map_key_for_filter, AppAction};

pub async fn run(hosts: Vec<Host>, args: Arc<ResolvedArgs>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Set up panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let result = run_app(&mut terminal, hosts, args).await;

    // Restore terminal
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    hosts: Vec<Host>,
    args: Arc<ResolvedArgs>,
) -> Result<()> {
    let mut app = App::new(hosts.clone(), args.warning_threshold, args.critical_threshold);
    let interval = args.interval;

    // Spawn SSH poller
    let mut rx = ssh::spawn_poller(hosts, args, interval);

    loop {
        // Draw
        terminal.draw(|f| ui::draw(f, &mut app))?;

        // Handle events (non-blocking with 50ms timeout for responsiveness)
        let action = if ct_event::poll(Duration::from_millis(50))? {
            if let ct_event::Event::Key(key) = ct_event::read()? {
                if app.show_help {
                    app.show_help = false;
                    AppAction::None
                } else if app.filter_mode {
                    map_key_for_filter(key)
                } else {
                    map_key_normal(key, &mut app)
                }
            } else {
                AppAction::None
            }
        } else {
            AppAction::None
        };

        // Process action
        match action {
            AppAction::Quit => {
                app.should_quit = true;
                break;
            }
            AppAction::MoveDown => app.move_down(),
            AppAction::MoveUp => app.move_up(),
            AppAction::PageDown => app.page_down(10),
            AppAction::PageUp => app.page_up(10),
            AppAction::Home => app.go_home(),
            AppAction::End => app.go_end(),
            AppAction::ToggleDetail => app.show_detail = !app.show_detail,
            AppAction::CycleSort => {
                app.sort_column = app.sort_column.next();
                app.refresh_visible();
            }
            AppAction::ReverseSort => {
                app.sort_ascending = !app.sort_ascending;
                app.refresh_visible();
            }
            AppAction::StartFilter => {
                app.filter_mode = true;
            }
            AppAction::FilterInput(c) => {
                app.filter_text.push(c);
                app.refresh_visible();
            }
            AppAction::FilterBackspace => {
                app.filter_text.pop();
                app.refresh_visible();
            }
            AppAction::CancelFilter => {
                app.filter_mode = false;
                app.filter_text.clear();
                app.refresh_visible();
            }
            AppAction::ConfirmFilter => {
                app.filter_mode = false;
            }
            AppAction::ForceRefresh => {}
            AppAction::ToggleHelp => {
                app.show_help = !app.show_help;
            }
            AppAction::None => {}
        }

        // Drain SSH messages in batch — insert directly, refresh once
        {
            let mut need_refresh = false;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SshMessage::Connecting(host_name) => {
                        app.set_connecting(&host_name);
                    }
                    SshMessage::Result(metrics) => {
                        app.last_poll = Some(std::time::Instant::now());
                        app.host_metrics.insert(metrics.host_name.clone(), metrics);
                        need_refresh = true;
                    }
                }
            }
            if need_refresh {
                app.refresh_visible();
            }
        }
    }

    Ok(())
}

fn map_key_normal(key: ct_event::KeyEvent, app: &mut App) -> AppAction {
    use ct_event::{KeyCode, KeyModifiers};

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => AppAction::Quit,
            KeyCode::Char('d') => AppAction::PageDown,
            KeyCode::Char('u') => AppAction::PageUp,
            _ => AppAction::None,
        };
    }

    match key.code {
        KeyCode::Char('q') => AppAction::Quit,
        KeyCode::Char('j') | KeyCode::Down => AppAction::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => AppAction::MoveUp,
        KeyCode::Char('g') => AppAction::Home,
        KeyCode::Char('G') => AppAction::End,
        KeyCode::Enter => AppAction::ToggleDetail,
        KeyCode::Char('s') => AppAction::CycleSort,
        KeyCode::Char('S') => AppAction::ReverseSort,
        KeyCode::Char('/') => AppAction::StartFilter,
        KeyCode::Char('r') => AppAction::ForceRefresh,
        KeyCode::Char('?') => AppAction::ToggleHelp,
        KeyCode::Esc => {
            if !app.filter_text.is_empty() {
                app.filter_text.clear();
                app.refresh_visible();
            }
            AppAction::None
        }
        KeyCode::PageDown => AppAction::PageDown,
        KeyCode::PageUp => AppAction::PageUp,
        KeyCode::Home => AppAction::Home,
        KeyCode::End => AppAction::End,
        _ => AppAction::None,
    }
}
