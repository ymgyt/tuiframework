use std::{
    io::{self, IsTerminal as _, Write},
    panic,
    sync::{Mutex, Once, PoisonError},
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
        enable_raw_mode, is_raw_mode_enabled,
    },
};

use super::TerminalOptions;

static ABORT_RECOVERY: Mutex<Option<TerminalOptions>> = Mutex::new(None);

/// Restores raw mode, the alternate screen, and the enabled input modes on drop.
#[must_use = "keep the guard alive while the terminal is in raw mode"]
pub(super) struct TerminalGuard {
    options: TerminalOptions,
}

impl TerminalGuard {
    pub(super) fn enable_raw_mode(options: TerminalOptions) -> io::Result<Self> {
        if options.restores_on_abort() {
            Self::install_abort_hook();
        }
        let output = io::stdout().lock();
        if !output.is_terminal() {
            return Err(io::Error::other("stdout is not a terminal"));
        }
        if is_raw_mode_enabled()? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "terminal raw mode is already in use",
            ));
        }
        let recovery = options.restores_on_abort().then(|| {
            ABORT_RECOVERY
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        });
        enable_raw_mode()?;
        if let Some(mut active) = recovery {
            *active = Some(options.clone());
        }
        Ok(Self { options })
    }

    pub(super) fn enter_alternate_screen(&self) -> io::Result<()> {
        let mut output = io::stdout().lock();
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

    fn install_abort_hook() {
        static INSTALL_HOOK: Once = Once::new();
        INSTALL_HOOK.call_once(|| {
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                Self::restore_after_abort();
                previous(info);
            }));
        });
    }

    fn restore_after_abort() {
        if ABORT_RECOVERY
            .try_lock()
            .is_ok_and(|active| active.is_none())
        {
            return;
        }
        // A terminal may have closed or reopened while the hook waited for stdout.
        let mut output = io::stdout().lock();
        if let Ok(active) = ABORT_RECOVERY.try_lock()
            && let Some(options) = active.as_ref()
        {
            Self::restore(&mut output, options);
        }
    }

    fn restore(output: &mut impl Write, options: &TerminalOptions) {
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

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut output = io::stdout().lock();
        Self::restore(&mut output, &self.options);
        if self.options.restores_on_abort() {
            *ABORT_RECOVERY
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
        }
    }
}
