use crate::{Effects, Ui};

pub(super) type OnStart<'a, U, D> = Box<dyn FnOnce(&mut U, &mut D) -> Effects<U> + 'a>;
pub(super) type Callback<'a, U, D> = Box<dyn FnOnce(&U, &mut D) + 'a>;

/// Callbacks for startup, a normal quit request, and shutdown.
///
/// A normal run invokes `on_start`, then `on_quit`, then `on_exit`.
/// When processing events manually, the started loop can still be inspected
/// after `on_quit`. Dropping it invokes `on_exit`.
#[must_use]
pub struct Hooks<'a, U: Ui, D> {
    pub(super) start: Option<OnStart<'a, U, D>>,
    pub(super) quit: Option<Callback<'a, U, D>>,
    pub(super) exit: Option<Callback<'a, U, D>>,
}

impl<U: Ui, D> Default for Hooks<'_, U, D> {
    fn default() -> Self {
        Self {
            start: None,
            quit: None,
            exit: None,
        }
    }
}

impl<'a, U: Ui, D> Hooks<'a, U, D> {
    /// Produces the initial effects before the first frame.
    pub fn on_start(mut self, callback: OnStart<'a, U, D>) -> Self {
        self.start = Some(callback);
        self
    }

    /// Runs when the UI requests quit or input reaches EOF, before the step returns.
    /// Errors and cancellation skip this hook.
    pub fn on_quit(mut self, callback: Callback<'a, U, D>) -> Self {
        self.quit = Some(callback);
        self
    }

    /// Runs when the started loop is dropped, after pending work is cancelled
    /// and the driver has shut down. This includes normal exit, errors, and cancellation.
    pub fn on_exit(mut self, callback: Callback<'a, U, D>) -> Self {
        self.exit = Some(callback);
        self
    }
}
