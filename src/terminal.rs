use std::{
    io::{self, IsTerminal as _, Stdout},
    sync::Mutex,
};

use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
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

static ACTIVE: Mutex<Option<TerminalOptions>> = Mutex::new(None);

/// Terminal input modes for a fullscreen session. Enable mouse capture for
/// applications that consume mouse events. Bracketed paste groups pasted text
/// into one paste event on terminals that support it. Both modes are opt-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalOptions {
    pub bracketed_paste: bool,
    pub mouse_capture: bool,
}

/// Owns raw mode and the alternate screen for the interactive entry point.
#[must_use = "keep the guard alive while the terminal is in raw mode"]
pub struct TerminalSession {
    options: TerminalOptions,
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        Self::with_options(TerminalOptions::default())
    }

    pub fn with_options(options: TerminalOptions) -> io::Result<Self> {
        if !io::stdout().is_terminal() {
            return Err(io::Error::other("stdout is not a terminal"));
        }
        #[cfg(panic = "abort")]
        Self::install_abort_hook();
        {
            let mut active = ACTIVE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active.is_some() {
                return Err(io::Error::other("a terminal session is already active"));
            }
            *active = Some(options);
        }
        let guard = Self { options };
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableFocusChange,
            Hide,
            Clear(ClearType::All)
        )?;
        if options.bracketed_paste {
            execute!(io::stdout(), EnableBracketedPaste)?;
        }
        if options.mouse_capture {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        Ok(guard)
    }

    pub(crate) fn terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        Terminal::new(CrosstermBackend::new(io::stdout()))
    }

    // With unwinding, the session guard restores the terminal. In a binary
    // built with panic=abort no destructor runs, so restore before the existing hook.
    #[cfg(panic = "abort")]
    fn install_abort_hook() {
        use std::{panic, sync::Once};
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                if let Ok(active) = ACTIVE.try_lock()
                    && let Some(options) = *active
                {
                    Self::restore(options);
                }
                previous(info);
            }));
        });
    }

    fn restore(options: TerminalOptions) {
        if options.mouse_capture {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
        if options.bracketed_paste {
            let _ = execute!(io::stdout(), DisableBracketedPaste);
        }
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableFocusChange, LeaveAlternateScreen, Show);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        Self::restore(self.options);
        *ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

pub(crate) fn draw<B: Backend>(
    terminal: &mut Terminal<B>,
    monochrome: bool,
    render: impl FnOnce(&mut Frame<'_>),
) -> Result<(), B::Error> {
    terminal.draw(|frame| {
        render(frame);
        if monochrome {
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
