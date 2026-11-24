use std::{error::Error, future::poll_fn, io, time::Duration};

use crossterm::event::{Event, EventStream};
use futures_util::{Stream, StreamExt as _};
use ratatui::{Terminal, backend::Backend};
use thiserror::Error;
use tokio::time::{self, Interval, MissedTickBehavior};

use crate::{
    component::{AppKeymap, Components},
    compositor::Compositor,
    driver::{Driver, DriverContext},
    terminal::{TerminalSession, draw, force_redraw},
};

/// An error from the input stream or the fullscreen terminal.
#[derive(Debug, Error)]
pub enum RunError<E: Error> {
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
    tick_interval: Duration,
    timer: Option<Interval>,
}

impl<C: Components, D: Driver<C>> Application<C, D> {
    pub fn new(components: C, driver: D, keymap: AppKeymap<C>) -> Self {
        Self {
            compositor: Compositor::new(components, keymap),
            driver,
            context: DriverContext::new(),
            started: false,
            tick_interval: Duration::from_millis(100),
            timer: None,
        }
    }

    #[must_use]
    pub fn with_tick_interval(mut self, period: Duration) -> Self {
        self.tick_interval = period.max(Duration::from_millis(1));
        self.timer = None;
        self
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

    /// Tests can wait for one-shot work to settle with long-lived watches installed.
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
        self.start();
        if self.context.take_redraw_request() {
            force_redraw(terminal)?;
        }
        draw(terminal, |frame| self.compositor.render(frame))
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
        let operations = tokio::select! {
            event = input.next() => {
                match event {
                    Some(Ok(event)) => self.compositor.input(&event),
                    Some(Err(error)) => { self.finish(); return Err(error); }
                    None => { self.finish(); return Ok(()); }
                }
            },
            event = self.context.next_event() => self.compositor.event(&event),
            event = poll_fn(|cx| self.driver.poll_event(cx)) => self.compositor.event(&event),
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
    pub async fn run(&mut self) -> Result<(), RunError<io::Error>> {
        let _guard = TerminalSession::enter().map_err(RunError::Terminal)?;
        let mut terminal = TerminalSession::terminal().map_err(RunError::Terminal)?;
        self.run_with(&mut terminal, &mut EventStream::new()).await
    }

    /// Uses an existing fullscreen terminal and a caller-supplied input stream.
    /// Returning or cancelling the run drops registered jobs and watches. The app
    /// state and the caller's terminal buffer remain available for inspection.
    pub async fn run_with<B, S>(
        &mut self,
        terminal: &mut Terminal<B>,
        input: &mut S,
    ) -> Result<(), RunError<B::Error>>
    where
        B: Backend,
        S: Stream<Item = io::Result<Event>> + Unpin,
    {
        let guard = RunGuard { app: self };
        guard.app.start();
        while !guard.app.is_finished() {
            guard.app.render(terminal).map_err(RunError::Terminal)?;
            guard.app.step(input).await.map_err(RunError::Input)?;
        }
        Ok(())
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

impl<C: Components, D: Driver<C>> Drop for RunGuard<'_, C, D> {
    fn drop(&mut self) {
        self.app.finish();
    }
}
