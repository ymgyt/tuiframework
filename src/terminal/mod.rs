mod device;
mod guard;
mod render_target;

use std::io::{self, Stdout};

use crossterm::style::Colored;
use ratatui::{Frame, backend::CrosstermBackend, style::Color};

pub use device::TerminalDevice;
use guard::TerminalGuard;
pub use render_target::RenderTarget;

/// Options for initializing a fullscreen terminal. Both input modes are off by default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalOptions {
    bracketed_paste: bool,
    mouse_capture: bool,
}

impl TerminalOptions {
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
}

/// A fullscreen terminal on stdout. Dropping it restores the terminal modes.
#[must_use = "dropping the terminal restores its input and display modes"]
pub struct Terminal<'d> {
    inner: ratatui::Terminal<CrosstermBackend<&'d Stdout>>,
    monochrome: bool,
    _guard: TerminalGuard<'d>,
}

impl<'d> Terminal<'d> {
    /// Enters raw mode and the alternate screen, with focus reporting enabled.
    ///
    /// Returns [`io::ErrorKind::AlreadyExists`] if this process already has an
    /// initialized terminal or Crossterm reports that raw mode is already enabled.
    ///
    /// ```no_run
    /// use tuiframework::{Terminal, TerminalDevice, TerminalOptions};
    ///
    /// let options = TerminalOptions::default().with_bracketed_paste(true);
    /// let mut device = TerminalDevice::stdout();
    /// device.install_abort_hook();
    /// let terminal = Terminal::init(&mut device, options)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn init(device: &'d mut TerminalDevice, options: TerminalOptions) -> io::Result<Self> {
        let guard = TerminalGuard::enable_raw_mode(device, options)?;
        guard.enter_alternate_screen()?;
        Ok(Self {
            inner: ratatui::Terminal::new(CrosstermBackend::new(device.output()))?,
            monochrome: Colored::ansi_color_disabled_memoized(),
            _guard: guard,
        })
    }
}

impl RenderTarget for Terminal<'_> {
    type Error = io::Error;

    fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        self.inner.draw(|frame| {
            render(frame);
            if self.monochrome {
                for cell in &mut frame.buffer_mut().content {
                    cell.fg = Color::Reset;
                    cell.bg = Color::Reset;
                    cell.underline_color = Color::Reset;
                }
            }
        })?;
        Ok(())
    }

    fn force_redraw(&mut self) -> io::Result<()> {
        self.inner.force_redraw()
    }
}
