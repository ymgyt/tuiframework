use std::io::{self, IsTerminal as _, Write};

use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture,
    },
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, is_raw_mode_enabled,
    },
};

use super::{TerminalDevice, TerminalOptions};

/// Restores raw mode, the alternate screen, and the enabled input modes on drop.
#[must_use = "keep the guard alive while the terminal is in raw mode"]
pub(super) struct TerminalGuard<'d> {
    device: &'d TerminalDevice,
    options: TerminalOptions,
}

impl<'d> TerminalGuard<'d> {
    pub(super) fn enable_raw_mode(
        device: &'d TerminalDevice,
        options: TerminalOptions,
    ) -> io::Result<Self> {
        let output = device.output().lock();
        if !output.is_terminal() {
            return Err(io::Error::other("stdout is not a terminal"));
        }
        if is_raw_mode_enabled()? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "terminal raw mode is already in use",
            ));
        }
        enable_raw_mode()?;
        device.set_recovery(Some(&options));
        Ok(Self { device, options })
    }

    pub(super) fn enter_alternate_screen(&self) -> io::Result<()> {
        let mut output = self.device.output().lock();
        execute!(
            output,
            EnterAlternateScreen,
            EnableFocusChange,
            Hide,
            Clear(ClearType::All)
        )?;
        if self.options.bracketed_paste {
            execute!(output, EnableBracketedPaste)?;
        }
        if self.options.mouse_capture {
            execute!(output, EnableMouseCapture)?;
        }
        Ok(())
    }

    pub(super) fn restore(output: &mut impl Write, options: &TerminalOptions) {
        if options.mouse_capture {
            let _ = execute!(output, DisableMouseCapture);
        }
        if options.bracketed_paste {
            let _ = execute!(output, DisableBracketedPaste);
        }
        let _ = disable_raw_mode();
        let _ = execute!(output, DisableFocusChange, LeaveAlternateScreen, Show);
    }
}

impl Drop for TerminalGuard<'_> {
    fn drop(&mut self) {
        let mut output = self.device.output().lock();
        Self::restore(&mut output, &self.options);
        self.device.set_recovery(None);
    }
}
