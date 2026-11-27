use std::{borrow::BorrowMut, error::Error, future::poll_fn, io, ops::ControlFlow};

use bon::bon;
use crossterm::event::Event;
use futures_util::{Stream, StreamExt as _};
use thiserror::Error;

use crate::{
    Compositor, DispatchResult, Driver, Effects, RenderTarget, Ui,
    driver::{DriverContext, DriverEvents, DriverMessage},
};

mod animation;
mod hooks;
pub use animation::AnimationTimer;
pub use hooks::Hooks;
use hooks::{Callback, OnStart};

pub(crate) enum Redraw {
    Diff,
    Full,
}

/// A configured application. Driving individual events requires consuming it with `start()`.
///
/// ```compile_fail,E0599
/// # use tuiframework::{Driver, EventLoop, Terminal, Ui, crossterm::event::EventStream, event_loop::Ready};
/// # fn example<U: Ui, D: Driver<Event = U::Event, Operation = U::Operation>>(
/// #     mut ready: EventLoop<Ready<'_, U, D>, Terminal, EventStream>) {
/// let _ = ready.process_next();
/// # }
/// ```
pub struct Ready<'a, U: Ui, D, C = Compositor<U>> {
    compositor: C,
    driver: D,
    hooks: Hooks<'a, U, D>,
}

/// Owns a started application until its driver and exit hook have finished.
pub struct Started<'a, U: Ui, D: Driver, C: BorrowMut<Compositor<U>> = Compositor<U>> {
    compositor: C,
    driver: D,
    events: DriverEvents<U::Event>,
    redraw: Redraw,
    control_flow: ControlFlow<()>,
    on_quit: Option<Callback<'a, U, D>>,
    on_exit: Option<Callback<'a, U, D>>,
}

/// Drives a UI on the given terminal and input. The state determines which operations are available.
pub struct EventLoop<S, T, I> {
    state: S,
    terminal: T,
    input: I,
    animation_timer: AnimationTimer,
}

impl<S, T, I> EventLoop<S, T, I> {
    pub fn terminal(&self) -> &T {
        &self.terminal
    }

    pub fn terminal_mut(&mut self) -> &mut T {
        &mut self.terminal
    }
}

#[bon]
impl<'a, U, D, C, T, I> EventLoop<Ready<'a, U, D, C>, T, I>
where
    U: Ui,
    D: Driver<Event = U::Event, Operation = U::Operation>,
    C: BorrowMut<Compositor<U>>,
    T: RenderTarget,
    I: Stream<Item = io::Result<Event>> + Unpin,
{
    /// Configures the event loop and its optional lifecycle hooks.
    #[builder]
    pub fn new(
        compositor: C,
        driver: D,
        terminal: T,
        input: I,
        #[builder(default)] animation_timer: AnimationTimer,
        #[builder(default)] hooks: Hooks<'a, U, D>,
    ) -> Self {
        Self {
            state: Ready {
                compositor,
                driver,
                hooks,
            },
            terminal,
            input,
            animation_timer,
        }
    }

    pub fn ui(&self) -> &U {
        &self.state.compositor.borrow().ui
    }

    pub fn driver(&self) -> &D {
        &self.state.driver
    }

    /// Consumes the configuration and applies the startup effects before the first frame.
    ///
    /// ```compile_fail,E0599
    /// # use tuiframework::{Driver, EventLoop, Terminal, Ui, crossterm::event::EventStream, event_loop::Ready};
    /// # fn example<U: Ui, D: Driver<Event = U::Event, Operation = U::Operation>>(
    /// #     ready: EventLoop<Ready<'_, U, D>, Terminal, EventStream>) {
    /// let started = ready.start();
    /// started.start();
    /// # }
    /// ```
    pub fn start(self) -> EventLoop<Started<'a, U, D, C>, T, I> {
        let Ready {
            compositor,
            driver,
            hooks,
        } = self.state;
        let mut started = EventLoop {
            state: Started {
                compositor,
                driver,
                events: DriverEvents::new(),
                redraw: Redraw::Diff,
                control_flow: ControlFlow::Continue(()),
                on_quit: hooks.quit,
                on_exit: hooks.exit,
            },
            terminal: self.terminal,
            input: self.input,
            animation_timer: self.animation_timer,
        };
        started.on_start(hooks.start);
        started
    }

    /// Starts on the first poll and drives the application until exit.
    pub async fn run(self) -> Result<(), RunError<T::Error>> {
        self.start().run().await
    }
}

impl<U, D, C, T, I> EventLoop<Started<'_, U, D, C>, T, I>
where
    U: Ui,
    D: Driver<Event = U::Event, Operation = U::Operation>,
    C: BorrowMut<Compositor<U>>,
    T: RenderTarget,
    I: Stream<Item = io::Result<Event>> + Unpin,
{
    fn on_start(&mut self, hook: Option<OnStart<'_, U, D>>) {
        if let Some(start) = hook {
            let compositor = self.state.compositor.borrow_mut();
            let effects = start(&mut compositor.ui, &mut self.state.driver);
            self.state.apply_effects(effects);
        }
    }

    pub fn ui(&self) -> &U {
        &self.state.compositor.borrow().ui
    }

    pub fn driver(&self) -> &D {
        &self.state.driver
    }

    /// Whether a quit request or an I/O error has stopped event processing.
    pub fn should_quit(&self) -> bool {
        self.state.control_flow.is_break()
    }

    /// Long-lived watches do not count as pending requests.
    pub fn has_pending_work(&self) -> bool {
        self.state.events.has_pending_work()
    }

    /// Delivers effects supplied by an embedding application while the UI accepts events.
    pub fn apply_effects(&mut self, effects: Effects<U>) {
        if !self.should_quit() {
            self.state.apply_effects(effects);
        }
    }

    /// Continues from the current application state. Dropping the future shuts it down.
    pub async fn run(mut self) -> Result<(), RunError<T::Error>> {
        while !self.should_quit() {
            self.render().map_err(RunError::Terminal)?;
            self.dispatch_next_event().await.map_err(RunError::Input)?;
        }
        Ok(())
    }

    /// Draws the current UI, including its final state if requested before drop.
    pub fn render(&mut self) -> Result<(), T::Error> {
        self.draw().inspect_err(|_| self.state.stop())
    }

    fn draw(&mut self) -> Result<(), T::Error> {
        if matches!(self.state.redraw, Redraw::Full) {
            self.terminal.force_redraw()?;
            self.state.redraw = Redraw::Diff;
        }
        let compositor = self.state.compositor.borrow();
        self.terminal.draw(|frame| compositor.render(frame))
    }

    /// Processes one event. Once the loop stops, returns immediately without polling input.
    pub async fn process_next(&mut self) -> io::Result<()> {
        if self.should_quit() {
            return Ok(());
        }
        self.dispatch_next_event().await
    }

    async fn dispatch_next_event(&mut self) -> io::Result<()> {
        let state = &mut self.state;
        tokio::select! {
            // Revisit biased polling if sustained input delays driver work or animation ticks.
            biased;
            input = self.input.next() => state.accept_input(input)?,
            event = state.events.next_event() => state.publish(&event),
            message = poll_fn(|cx| state.driver.poll(cx)) => match message {
                DriverMessage::Event(event) => state.publish(&event),
                DriverMessage::Operation(operation) => state.dispatch(operation),
            },
            () = self.animation_timer.tick(), if state.compositor.borrow().ui.is_busy() => {
                state.compositor.borrow_mut().tick();
            }
        }
        Ok(())
    }
}

impl<U, D, C> Started<'_, U, D, C>
where
    U: Ui,
    D: Driver<Event = U::Event, Operation = U::Operation>,
    C: BorrowMut<Compositor<U>>,
{
    fn accept_input(&mut self, input: Option<io::Result<Event>>) -> io::Result<()> {
        match input {
            Some(Ok(input)) => {
                let result = self.compositor.borrow_mut().apply_terminal_event(&input);
                self.dispatch_result(result);
                Ok(())
            }
            Some(Err(error)) => {
                self.stop();
                Err(error)
            }
            None => {
                self.quit();
                Ok(())
            }
        }
    }

    fn publish(&mut self, event: &U::Event) {
        let result = self.compositor.borrow_mut().apply_event(event);
        self.dispatch_result(result);
    }

    fn apply_effects(&mut self, effects: Effects<U>) {
        let result = self.compositor.borrow_mut().apply_effects(effects);
        self.dispatch_result(result);
    }

    fn dispatch_result(&mut self, result: DispatchResult<U::Operation>) {
        for operation in result.operations {
            self.dispatch(operation);
        }
        if result.control_flow.is_break() {
            self.quit();
        }
    }

    fn dispatch(&mut self, operation: U::Operation) {
        self.driver.dispatch(
            operation,
            &mut DriverContext::new(&mut self.events, &mut self.redraw),
        );
    }

    fn stop(&mut self) {
        self.control_flow = ControlFlow::Break(());
        self.events.cancel();
    }

    fn quit(&mut self) {
        self.stop();
        if let Some(quit) = self.on_quit.take() {
            quit(&self.compositor.borrow().ui, &mut self.driver);
        }
    }
}

impl<U: Ui, D: Driver, C: BorrowMut<Compositor<U>>> Drop for Started<'_, U, D, C> {
    fn drop(&mut self) {
        if self.control_flow.is_continue() {
            self.events.cancel();
        }
        self.driver.shutdown();
        if let Some(exit) = self.on_exit.take() {
            exit(&self.compositor.borrow().ui, &mut self.driver);
        }
    }
}

/// An input or rendering error encountered while driving the event loop.
#[derive(Debug, Error)]
pub enum RunError<E: Error> {
    #[error("terminal input: {0}")]
    Input(#[source] io::Error),
    #[error("terminal output: {0}")]
    Terminal(#[source] E),
}

impl From<RunError<io::Error>> for io::Error {
    fn from(error: RunError<Self>) -> Self {
        match error {
            RunError::Input(error) | RunError::Terminal(error) => error,
        }
    }
}
