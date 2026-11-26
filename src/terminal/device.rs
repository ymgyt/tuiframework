use std::{
    io::{self, Stdout},
    panic,
    sync::{Arc, Mutex, PoisonError},
};

use super::{TerminalOptions, guard::TerminalGuard};

/// The stdout device. A fullscreen terminal borrows it until its modes are restored.
pub struct TerminalDevice {
    stdout: Stdout,
    recovery: Option<Arc<Mutex<Option<TerminalOptions>>>>,
}

impl TerminalDevice {
    /// Obtains stdout without changing terminal modes or the process panic hook.
    pub fn stdout() -> Self {
        Self {
            stdout: io::stdout(),
            recovery: None,
        }
    }

    /// In a panic-abort binary, restores this device before the previous panic hook runs.
    /// Install once on the device that the application reuses for its terminal.
    /// The hook stays installed after the device drops and then only delegates.
    pub fn install_abort_hook(&mut self) {
        if cfg!(panic = "abort") && self.recovery.is_none() {
            let recovery = Arc::new(Mutex::new(None));
            let active = Arc::downgrade(&recovery);
            let previous = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                if let Some(active) = active.upgrade() {
                    // Serialize recovery with terminal initialization and restoration.
                    let mut output = io::stdout().lock();
                    if let Ok(options) = active.try_lock()
                        && let Some(options) = options.as_ref()
                    {
                        TerminalGuard::restore(&mut output, options);
                    }
                }
                previous(info);
            }));
            self.recovery = Some(recovery);
        }
    }

    pub(super) fn output(&self) -> &Stdout {
        &self.stdout
    }

    pub(super) fn set_recovery(&self, options: Option<&TerminalOptions>) {
        if let Some(recovery) = &self.recovery {
            *recovery.lock().unwrap_or_else(PoisonError::into_inner) = options.cloned();
        }
    }
}
