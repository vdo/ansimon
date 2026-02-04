use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

/// Actions that can be dispatched from key events.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    Quit,
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    Home,
    End,
    ToggleDetail,
    CycleSort,
    ReverseSort,
    StartFilter,
    FilterInput(char),
    FilterBackspace,
    CancelFilter,
    ConfirmFilter,
    ForceRefresh,
    ToggleHelp,
    None,
}

/// Map a key event to an AppAction in filter mode.
pub fn map_key_for_filter(key: KeyEvent) -> AppAction {
    match key.code {
        KeyCode::Esc => AppAction::CancelFilter,
        KeyCode::Enter => AppAction::ConfirmFilter,
        KeyCode::Backspace => AppAction::FilterBackspace,
        KeyCode::Char(c) => AppAction::FilterInput(c),
        _ => AppAction::None,
    }
}
