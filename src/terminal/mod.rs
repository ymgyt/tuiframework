mod guard;
mod render_target;

use std::io::{self, Stdout};

use crossterm::style::Colored;
use ratatui::{Frame, backend::CrosstermBackend, style::Color};

use guard::TerminalGuard;
pub use render_target::RenderTarget;

/// Options for initializing a fullscreen terminal. All options are off by default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalOptions {
    bracketed_paste: bool,
    mouse_capture: bool,
    restore_on_panic_abort: bool,
}

impl TerminalOptions {
    fn restores_on_abort(&self) -> bool {
        cfg!(panic = "abort") && self.restore_on_panic_abort
    }

    /// Reports pasted text as a single paste event on supporting terminals.
    #[must_use]
    pub fn with_bracketed_paste(mut self, enabled: bool) -> Self {
        self.bracketed_paste = enabled;
        self
    }

    /// Reports mouse input as terminal events.
    #[must_use]
    pub fn with_mouse_capture(mut self, enabled: bool) -> Self {
        self.mouse_capture = enabled;
        self
    }

    /// In a `panic = "abort"` build, restores the active terminal before the previous
    /// panic hook runs. The process-wide hook is installed once and stays installed.
    /// This option has no effect in builds that unwind on panic.
    #[must_use]
    pub fn with_restore_on_panic_abort(mut self, enabled: bool) -> Self {
        self.restore_on_panic_abort = enabled;
        self
    }
}

/// A fullscreen terminal on stdout. Dropping it restores the terminal modes.
#[must_use = "dropping the terminal restores its input and display modes"]
pub struct Terminal {
    inner: ratatui::Terminal<CrosstermBackend<Stdout>>,
    monochrome: bool,
    _guard: TerminalGuard,
}

impl Terminal {
    /// Enters raw mode and the alternate screen, with focus reporting enabled.
    ///
    /// Returns [`io::ErrorKind::AlreadyExists`] if this process already has an
    /// initialized terminal or Crossterm reports that raw mode is already enabled.
    ///
    /// ```no_run
    /// use tuiframework::{Terminal, TerminalOptions};
    ///
    /// let options = TerminalOptions::default()
    ///     .with_bracketed_paste(true)
    ///     .with_restore_on_panic_abort(true);
    /// let terminal = Terminal::init(options)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn init(options: TerminalOptions) -> io::Result<Self> {
        let guard = TerminalGuard::enable_raw_mode(options)?;
        guard.enter_alternate_screen()?;
        Ok(Self {
            inner: ratatui::Terminal::new(CrosstermBackend::new(io::stdout()))?,
            monochrome: Colored::ansi_color_disabled_memoized(),
            _guard: guard,
        })
    }
}

impl RenderTarget for Terminal {
    type Error = io::Error;

    fn render(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        self.inner.render(|frame| {
            render(frame);
            if self.monochrome {
                for cell in &mut frame.buffer_mut().content {
                    cell.fg = Color::Reset;
                    cell.bg = Color::Reset;
                    cell.underline_color = Color::Reset;
                }
            }
        })
    }

    fn force_redraw(&mut self) -> io::Result<()> {
        self.inner.force_redraw()
    }
}
