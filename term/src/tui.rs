//! Terminal init and restore (raw mode, alternate screen, keyboard enhancement, panic hook).

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};

pub type Backend = CrosstermBackend<Stdout>;

static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

/// Enables raw mode + the alternate screen, pushes Kitty keyboard
/// enhancement flags when the terminal supports them, and installs a panic
/// hook so a panic never leaves the terminal stuck in any of those modes.
pub fn init() -> io::Result<Terminal<Backend>> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    if supports_keyboard_enhancement().unwrap_or(false) {
        let pushed = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
        .is_ok();
        KEYBOARD_ENHANCED.store(pushed, Ordering::SeqCst);
    }

    install_panic_hook();

    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

/// Best-effort terminal restore: pops keyboard enhancement flags (if they
/// were pushed), leaves the alternate screen, disables raw mode. Each step
/// is attempted independently so one failure doesn't skip the rest.
pub fn restore() {
    if KEYBOARD_ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
