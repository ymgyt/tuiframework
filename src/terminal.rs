use std::{
    io::{self, IsTerminal as _, Stdout},
    panic,
    sync::{
        Once,
        atomic::{AtomicBool, Ordering},
    },
};

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableFocusChange, EnableFocusChange},
    execute,
    style::Colored,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, ClearType as BackendClearType, CrosstermBackend},
    style::Color,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();

/// Owns raw mode and the alternate screen for the interactive entry point.
#[must_use = "keep the guard alive while the terminal is in raw mode"]
pub struct TerminalSession {
    _private: (),
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        if !io::stdout().is_terminal() {
            return Err(io::Error::other("stdout is not a terminal"));
        }
        PANIC_HOOK.call_once(|| {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                if ACTIVE.load(Ordering::Acquire) {
                    Self::restore();
                }
                previous(info);
            }));
        });
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| io::Error::other("a terminal session is already active"))?;
        let guard = Self { _private: () };
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableFocusChange,
            Hide,
            Clear(ClearType::All)
        )?;
        Ok(guard)
    }

    pub(crate) fn terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        Terminal::new(CrosstermBackend::new(io::stdout()))
    }

    fn restore() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableFocusChange, LeaveAlternateScreen, Show);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        Self::restore();
        ACTIVE.store(false, Ordering::Release);
    }
}

pub(crate) fn draw<B: Backend>(
    terminal: &mut Terminal<B>,
    render: impl FnOnce(&mut Frame<'_>),
) -> Result<(), B::Error> {
    terminal.draw(|frame| {
        render(frame);
        if Colored::ansi_color_disabled_memoized() {
            // Crossterm color changes can reset bold and dim under NO_COLOR.
            for cell in &mut frame.buffer_mut().content {
                cell.fg = Color::Reset;
                cell.bg = Color::Reset;
                cell.underline_color = Color::Reset;
            }
        }
    })?;
    Ok(())
}

pub(crate) fn force_redraw<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), B::Error> {
    // Terminal::clear queries the cursor position, which can block on a real TTY.
    terminal.backend_mut().clear_region(BackendClearType::All)?;
    terminal.swap_buffers();
    Ok(())
}
