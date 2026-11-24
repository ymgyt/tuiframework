use std::{
    error::Error,
    future::{Future, poll_fn},
    io,
    task::Poll,
    time::Duration,
};

use crossterm::{
    event::{Event, EventStream},
    style::Colored,
};
use futures_util::{Stream, StreamExt as _};
use ratatui::{Terminal, backend::Backend};
use thiserror::Error;
use tokio::time::{self, Interval, MissedTickBehavior};

use crate::{
    component::Components,
    compositor::Compositor,
    driver::{Driver, DriverContext, DriverMessage},
    terminal::{TerminalOptions, TerminalSession, draw, force_redraw},
};

const INPUT_BURST: u8 = 16;

/// An error from the input stream or the fullscreen terminal.
#[derive(Debug, Error)]
pub enum RunError<E: Error> {
    #[error("terminal setup: {0}")]
    Setup(#[source] io::Error),
    #[error("terminal input: {0}")]
    Input(#[source] io::Error),
    #[error("terminal output: {0}")]
    Terminal(#[source] E),
}

/// Runs a component store and its driver on a fullscreen terminal.
pub struct Application<C: Components, D: Driver<C>> {
    compositor: Compositor<C>,
    driver: D,
    context: DriverContext<C::Event>,
    started: bool,
    consecutive_inputs: u8,
    terminal_options: TerminalOptions,
    tick_interval: Duration,
    timer: Option<Interval>,
}

#[bon::bon]
impl<C: Components, D: Driver<C>> Application<C, D> {
    /// Wires the component store, its driver, and the resolved keymap.
    #[builder]
    pub fn new(
        components: C,
        driver: D,
        keymap: C::Keymap,
        #[builder(default)] terminal_options: TerminalOptions,
        #[builder(default = Duration::from_millis(100))] tick_interval: Duration,
    ) -> Self {
        Self {
            compositor: Compositor::new(components, keymap),
            driver,
            context: DriverContext::new(),
            started: false,
            consecutive_inputs: 0,
            terminal_options,
            tick_interval: tick_interval.max(Duration::from_millis(1)),
            timer: None,
        }
    }

    pub fn components(&self) -> &C {
        &self.compositor.components
    }
    pub fn driver(&self) -> &D {
        &self.driver
    }
    pub fn is_finished(&self) -> bool {
        self.compositor.quit
    }

    /// Whether work registered with the driver context still has an outstanding
    /// request or queued event. Long-lived watches are excluded.
    pub fn has_pending_work(&self) -> bool {
        self.context.has_pending_work()
    }

    fn start(&mut self) {
        if !self.started && !self.is_finished() {
            self.started = true;
            let ops = self.compositor.start();
            self.dispatch_all(ops);
        }
    }

    /// Delivers an addressed command and dispatches its effects, including
    /// before the first frame when opening a particular screen on startup.
    pub fn send(&mut self, to: C::Id, command: C::Command) {
        self.start();
        if !self.is_finished() {
            let operations = self.compositor.send(to, command);
            self.dispatch_all(operations);
        }
    }

    /// Delivers an operation directly to the driver.
    pub fn dispatch(&mut self, operation: C::Operation) {
        self.start();
        if !self.is_finished() {
            self.driver.dispatch(operation, &mut self.context);
        }
    }

    fn dispatch_all(&mut self, operations: Vec<C::Operation>) {
        for op in operations {
            self.driver.dispatch(op, &mut self.context);
        }
        if self.is_finished() {
            self.finish();
        }
    }

    /// Applies a fact to its subscribers and dispatches the resulting operations.
    pub fn publish(&mut self, event: &C::Event) {
        self.start();
        if !self.is_finished() {
            let ops = self.compositor.event(event);
            self.dispatch_all(ops);
        }
    }

    pub fn render<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), B::Error> {
        self.draw_frame(terminal, false)
    }

    fn draw_frame<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        monochrome: bool,
    ) -> Result<(), B::Error> {
        self.start();
        if self.context.take_redraw_request() {
            force_redraw(terminal)?;
        }
        draw(terminal, monochrome, |frame| self.compositor.render(frame))
    }

    fn accept_input(&mut self, input: Option<io::Result<Event>>) -> io::Result<()> {
        match input {
            Some(Ok(event)) => {
                let operations = self.compositor.input(&event);
                self.dispatch_all(operations);
                Ok(())
            }
            Some(Err(error)) => {
                self.finish();
                Err(error)
            }
            None => {
                self.finish();
                Ok(())
            }
        }
    }

    /// Applies one terminal input, driver result, or animation tick. EOF ends the run.
    pub async fn step<S>(&mut self, input: &mut S) -> io::Result<()>
    where
        S: Stream<Item = io::Result<Event>> + Unpin,
    {
        self.start();
        if self.is_finished() {
            return Ok(());
        }
        // Honor keys the terminal has already delivered before background
        // completions. A sustained input stream still gives drivers and timers turns.
        if self.consecutive_inputs < INPUT_BURST
            && let Poll::Ready(event) = poll_fn(|cx| Poll::Ready(input.poll_next_unpin(cx))).await
        {
            self.consecutive_inputs += 1;
            return self.accept_input(event);
        }
        self.consecutive_inputs = 0;
        let operations = tokio::select! {
            event = input.next() => return self.accept_input(event),
            event = self.context.next_event() => self.compositor.event(&event),
            message = poll_fn(|cx| self.driver.poll(cx)) => match message {
                DriverMessage::Event(event) => self.compositor.event(&event),
                DriverMessage::Operation(op) => vec![op],
            },
            () = async {
                let timer = self.timer.get_or_insert_with(|| {
                    let mut timer = time::interval(self.tick_interval);
                    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    timer
                });
                timer.tick().await;
            }, if self.compositor.components.is_busy() => {
                self.compositor.components.tick();
                self.compositor.refresh();
                Vec::new()
            }
        };
        self.dispatch_all(operations);
        Ok(())
    }

    /// Runs on stdout in raw mode and restores the terminal on exit or cancellation.
    pub fn run(&mut self) -> impl Future<Output = Result<(), RunError<io::Error>>> + '_ {
        let guard = RunGuard { app: self };
        async move {
            let _session = TerminalSession::with_options(guard.app.terminal_options)
                .map_err(RunError::Setup)?;
            let mut terminal = TerminalSession::terminal().map_err(RunError::Setup)?;
            guard
                .run_loop(
                    &mut terminal,
                    &mut EventStream::new(),
                    Colored::ansi_color_disabled_memoized(),
                )
                .await
        }
    }

    /// Uses the caller's terminal and input modes and preserves the rendered cell
    /// styles. Dropping the returned future stops outstanding work, even before
    /// its first poll. The terminal buffer remains available for inspection.
    pub fn run_with<'a, B, S>(
        &'a mut self,
        terminal: &'a mut Terminal<B>,
        input: &'a mut S,
    ) -> impl Future<Output = Result<(), RunError<B::Error>>> + 'a
    where
        B: Backend,
        S: Stream<Item = io::Result<Event>> + Unpin,
    {
        RunGuard { app: self }.run_loop(terminal, input, false)
    }

    fn finish(&mut self) {
        self.compositor.quit = true;
        if std::mem::take(&mut self.started) {
            self.timer = None;
            self.context = DriverContext::new();
            self.driver.shutdown();
        }
    }
}

impl<C: Components, D: Driver<C>> Drop for Application<C, D> {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Cancels work even when the owner drops the run future and retains the application.
struct RunGuard<'a, C: Components, D: Driver<C>> {
    app: &'a mut Application<C, D>,
}

impl<C: Components, D: Driver<C>> RunGuard<'_, C, D> {
    async fn run_loop<B, S>(
        self,
        terminal: &mut Terminal<B>,
        input: &mut S,
        monochrome: bool,
    ) -> Result<(), RunError<B::Error>>
    where
        B: Backend,
        S: Stream<Item = io::Result<Event>> + Unpin,
    {
        self.app.start();
        while !self.app.is_finished() {
            self.app
                .draw_frame(terminal, monochrome)
                .map_err(RunError::Terminal)?;
            self.app.step(input).await.map_err(RunError::Input)?;
        }
        Ok(())
    }
}

impl<C: Components, D: Driver<C>> Drop for RunGuard<'_, C, D> {
    fn drop(&mut self) {
        self.app.finish();
    }
}
