use std::{cell::RefCell, future::pending, io, rc::Rc};

use futures_util::{FutureExt as _, stream};
use tokio::sync::oneshot;
use tuiframework::{
    AnimationTimer, Component, Components, Driver, DriverContext, Effects, EventLoop, Hooks,
    KeyAction, Overlay, RenderInputs, RenderTarget, RunError, UiKeymap,
    crossterm::event::Event as Input,
    event_loop::Ready,
    keymap::{Binding, KeySequence, KeyStroke, KeymapConfig},
    ratatui::{Frame, Terminal, backend::TestBackend, widgets::Paragraph},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Id {
    Page,
    Observer,
}
enum Event {
    Changed,
    FollowUp,
}
struct Command;
struct Hold;

#[derive(Clone)]
struct NotifyObserver;

impl KeyAction<App> for NotifyObserver {
    fn command_for(self, owner: Id) -> (Id, Command) {
        assert!(matches!(owner, Id::Page));
        (Id::Observer, Command)
    }
}

#[derive(Clone)]
struct Pane {
    animating: bool,
    id: Id,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl Component<App> for Pane {
    fn key_layers(&self) -> &[()] {
        &[()]
    }

    fn render(&self, frame: &mut Frame<'_>, _: &RenderInputs<'_, App>) {
        frame.render_widget(Paragraph::new("visible"), frame.area());
        self.trace.borrow_mut().push("render");
    }

    fn subscribes(&self, event: &Event) -> bool {
        matches!(self.id, Id::Observer) || matches!(event, Event::Changed)
    }

    fn apply_event(&mut self, event: &Event, effects: &mut Effects<App>) {
        match (self.id, event) {
            (Id::Page, Event::Changed) => {
                self.trace.borrow_mut().push("page changed");
                effects.emit(Event::FollowUp);
            }
            (Id::Observer, Event::FollowUp) => {
                self.trace.borrow_mut().push("observer follow-up");
            }
            (Id::Observer, Event::Changed) => {
                self.trace.borrow_mut().push("observer changed");
            }
            _ => unreachable!(),
        }
    }

    fn apply_command(&mut self, _: Command, _: &mut Effects<App>) {
        assert!(matches!(self.id, Id::Observer));
        self.trace.borrow_mut().push("observer command");
    }
    fn is_busy(&self) -> bool {
        self.animating
    }
    fn tick(&mut self) {
        self.trace.borrow_mut().push("tick");
    }
}

#[derive(Clone)]
struct App {
    page: Pane,
    observer: Pane,
}

impl Components for App {
    type Id = Id;
    type Layer = ();
    type Command = Command;
    type OverlayId = ();
    type StackId = ();
    type Keymap = UiKeymap<Self, NotifyObserver>;
    type Event = Event;
    type Operation = Hold;
    type RenderContext<'a> = RenderInputs<'a, Self>;
    fn ids(&self) -> impl IntoIterator<Item = Id> {
        [Id::Page, Id::Observer]
    }
    fn get(&self, id: Id) -> Option<&dyn Component<Self>> {
        Some(match id {
            Id::Page => &self.page,
            Id::Observer => &self.observer,
        })
    }
    fn get_mut(&mut self, id: Id) -> Option<&mut dyn Component<Self>> {
        Some(match id {
            Id::Page => &mut self.page,
            Id::Observer => &mut self.observer,
        })
    }
}

struct Jobs {
    sender: Option<oneshot::Sender<()>>,
}
impl Driver for Jobs {
    type Event = Event;
    type Operation = Hold;
    fn dispatch(&mut self, _: Hold, cx: &mut DriverContext<Event>) {
        let guard = self.sender.take().unwrap();
        cx.submit(async move {
            let _held = guard;
            pending().await
        });
    }
}

struct TestApp {
    screens: App,
    jobs: Jobs,
    quits: usize,
    exits: usize,
}

impl TestApp {
    fn event_loop<T, I>(
        &mut self,
        terminal: T,
        input: I,
    ) -> EventLoop<Ready<'_, App, &mut Jobs>, T, I>
    where
        T: RenderTarget,
        I: futures_util::Stream<Item = io::Result<Input>> + Unpin,
    {
        let quits = &mut self.quits;
        let exits = &mut self.exits;
        EventLoop::builder()
            .components(self.screens.clone())
            .overlay(Overlay::new().component(Id::Page))
            .keymap(keys())
            .driver(&mut self.jobs)
            .terminal(terminal)
            .input(input)
            .animation_timer(AnimationTimer::new(std::time::Duration::from_secs(2)))
            .hooks(
                Hooks::default()
                    .on_start(Box::new(|_, _, effects| {
                        effects.emit(Event::Changed);
                        effects.operation(Hold);
                    }))
                    .on_quit(Box::new(|_, _| *quits += 1))
                    .on_exit(Box::new(|_, _| *exits += 1)),
            )
            .build()
    }
}

fn keys() -> UiKeymap<App, NotifyObserver> {
    let mut keys = KeymapConfig::default();
    keys.bind(
        (),
        Binding {
            on: KeySequence::parse(["x"]).unwrap(),
            command: NotifyObserver,
            desc: None,
        },
    );
    UiKeymap::<App, NotifyObserver>::compile(keys).unwrap()
}

fn build_app(animating: bool) -> (TestApp, oneshot::Receiver<()>) {
    let (sender, receiver) = oneshot::channel();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let app = App {
        page: Pane {
            animating,
            id: Id::Page,
            trace: Rc::clone(&trace),
        },
        observer: Pane {
            animating: false,
            id: Id::Observer,
            trace,
        },
    };
    (
        TestApp {
            screens: app,
            jobs: Jobs {
                sender: Some(sender),
            },
            quits: 0,
            exits: 0,
        },
        receiver,
    )
}

#[tokio::test]
async fn a_key_action_can_address_another_component_with_a_non_clone_command() {
    let (mut app, _) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut input = stream::iter([Ok(Input::Key(KeyStroke::from_char('x').into()))]);
    app.event_loop(&mut terminal, &mut input)
        .run()
        .await
        .unwrap();
    assert!(
        app.screens
            .page
            .trace
            .borrow()
            .ends_with(&["render", "observer command", "render"])
    );
}

#[tokio::test]
async fn hidden_subscribers_observe_nested_facts_before_eof_cancels_work() {
    let (mut application, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    application
        .event_loop(&mut terminal, stream::empty())
        .run()
        .await
        .unwrap();
    assert_eq!(application.exits, 1);
    assert_eq!(
        *application.screens.page.trace.borrow(),
        [
            "page changed",
            "observer follow-up",
            "observer changed",
            "render",
        ]
    );
    terminal.backend().assert_buffer_lines(["visible   "]);
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[tokio::test]
async fn cancelling_the_loop_and_propagating_input_errors_both_drop_pending_work() {
    let (mut application, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut input = stream::pending();
    {
        let mut run = Box::pin(application.event_loop(&mut terminal, &mut input).run());
        assert!(run.as_mut().now_or_never().is_none());
        assert_eq!(dropped.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    }
    assert_eq!((application.quits, application.exits), (0, 1));
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );

    let (mut application, mut dropped) = build_app(false);
    let mut input = stream::iter([Err(io::Error::other("input failed"))]);
    let error = application
        .event_loop(&mut terminal, &mut input)
        .run()
        .await
        .unwrap_err();
    assert!(matches!(error, RunError::Input(e) if e.to_string() == "input failed"));
    assert_eq!((application.quits, application.exits), (0, 1));
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[test]
fn a_full_render_request_repaints_only_the_next_frame() {
    use tuiframework::ratatui::backend::Backend as _;

    struct Repaint;
    impl Driver for Repaint {
        type Event = Event;
        type Operation = Hold;
        fn dispatch(&mut self, _: Hold, cx: &mut DriverContext<Event>) {
            cx.force_redraw();
        }
    }

    let (app, _) = build_app(false);
    let mut events = EventLoop::builder()
        .components(app.screens)
        .overlay(Overlay::new().component(Id::Page))
        .keymap(keys())
        .driver(Repaint)
        .terminal(Terminal::new(TestBackend::new(10, 1)).unwrap())
        .input(stream::pending())
        .build()
        .start();
    events.render().unwrap();

    // Simulate an external program clearing the screen behind Ratatui's back.
    events.terminal_mut().backend_mut().clear().unwrap();
    events.apply_effects(|effects| effects.operation(Hold));
    events.render().unwrap();
    events
        .terminal()
        .backend()
        .assert_buffer_lines(["visible   "]);

    events.terminal_mut().backend_mut().clear().unwrap();
    events.render().unwrap();
    events
        .terminal()
        .backend()
        .assert_buffer_lines(["          "]);
}

#[test]
fn preview_render_does_not_start_the_application() {
    let (app, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let preview = tuiframework::testing::TestScreen::new(
        app.screens.clone(),
        Overlay::new().component(Id::Page),
        keys(),
    );
    terminal.render(|frame| preview.render(frame)).unwrap();
    assert_eq!(*app.screens.page.trace.borrow(), ["render"]);
    assert!(app.jobs.sender.is_some());
    assert_eq!(dropped.try_recv(), Err(oneshot::error::TryRecvError::Empty));
}

#[tokio::test(start_paused = true)]
async fn only_busy_components_wake_the_loop_on_the_configured_interval() {
    use std::time::Duration;

    let (mut idle_app, _) = build_app(false);
    let (mut busy_app, _) = build_app(true);
    let mut idle = idle_app
        .event_loop(
            Terminal::new(TestBackend::new(10, 1)).unwrap(),
            stream::pending(),
        )
        .start();
    let mut busy = busy_app
        .event_loop(
            Terminal::new(TestBackend::new(10, 1)).unwrap(),
            stream::pending(),
        )
        .start();
    let period = Duration::from_secs(2);
    busy.process_next().await.unwrap();
    tokio::time::advance(period / 2).await;
    assert!(busy.process_next().now_or_never().is_none());
    tokio::time::advance(period / 2).await;
    busy.process_next().await.unwrap();
    assert!(idle.process_next().now_or_never().is_none());
    assert!(
        busy.components()
            .page
            .trace
            .borrow()
            .ends_with(&["tick", "tick"])
    );
}

#[tokio::test]
async fn dropping_an_unpolled_run_stops_work_started_before_the_first_frame() {
    let (mut app, mut dropped) = build_app(false);
    let terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut events = app.event_loop(terminal, stream::pending()).start();
    events.apply_command(Id::Observer, Command);
    drop(events.run());
    assert_eq!(app.exits, 1);
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[test]
fn building_and_dropping_a_loop_or_an_unpolled_run_leaves_the_application_untouched() {
    let (mut app, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    drop(app.event_loop(&mut terminal, stream::empty()));
    drop(app.event_loop(&mut terminal, stream::empty()).run());
    assert_eq!(app.exits, 0);
    assert!(app.screens.page.trace.borrow().is_empty());
    assert!(app.jobs.sender.is_some());
    assert_eq!(dropped.try_recv(), Err(oneshot::error::TryRecvError::Empty));
}

#[tokio::test]
async fn eof_stops_processing_before_the_started_loop_is_dropped() {
    let (app, mut dropped) = build_app(false);
    let notifications = RefCell::new(Vec::new());
    let terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let ready = EventLoop::builder()
        .components(app.screens)
        .overlay(Overlay::new().component(Id::Page))
        .keymap(keys())
        .driver(app.jobs)
        .terminal(terminal)
        .input(stream::empty())
        .hooks(
            Hooks::default()
                .on_start(Box::new(|_, _, effects| {
                    notifications.borrow_mut().push("start");
                    effects.operation(Hold);
                }))
                .on_quit(Box::new(|_, _| notifications.borrow_mut().push("quit")))
                .on_exit(Box::new(|_, _| notifications.borrow_mut().push("exit"))),
        )
        .build();
    assert!(notifications.borrow().is_empty());
    let mut events = ready.start();
    assert!(events.has_pending_work());
    events.process_next().await.unwrap();
    assert_eq!(*notifications.borrow(), ["start", "quit"]);
    assert!(events.should_quit());
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );

    events.process_next().now_or_never().unwrap().unwrap();
    events.apply_effects(|effects| effects.operation(Hold));
    events.run().await.unwrap();
    assert_eq!(*notifications.borrow(), ["start", "quit", "exit"]);
}

#[test]
fn a_render_error_stops_requests_then_the_driver_then_the_application() {
    use std::sync::{Arc, Mutex};

    struct Offline;
    impl RenderTarget for Offline {
        type Error = io::Error;
        fn render(&mut self, _: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "terminal closed"))
        }
        fn force_redraw(&mut self) -> io::Result<()> {
            unreachable!()
        }
    }

    struct Request(Arc<Mutex<Vec<&'static str>>>);
    impl Drop for Request {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("request dropped");
        }
    }

    struct DriverWithRequest(Arc<Mutex<Vec<&'static str>>>);
    impl Driver for DriverWithRequest {
        type Event = Event;
        type Operation = Hold;
        fn dispatch(&mut self, _: Hold, cx: &mut DriverContext<Event>) {
            let request = Request(Arc::clone(&self.0));
            cx.submit(async move {
                let _request = request;
                pending().await
            });
        }
        fn shutdown(&mut self) {
            self.0.lock().unwrap().push("driver stopped");
        }
    }

    let (app, _) = build_app(false);
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut running = EventLoop::builder()
        .components(app.screens)
        .overlay(Overlay::new().component(Id::Page))
        .keymap(keys())
        .driver(DriverWithRequest(Arc::clone(&trace)))
        .terminal(Offline)
        .input(stream::pending())
        .hooks(
            Hooks::default()
                .on_start(Box::new(|_, _, effects| effects.operation(Hold)))
                .on_quit(Box::new(|_, _| {
                    panic!("a terminal error is not a quit request")
                }))
                .on_exit(Box::new(|_, _| trace.lock().unwrap().push("app exited"))),
        )
        .build()
        .start();
    assert_eq!(
        running.render().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(running.should_quit());
    running.process_next().now_or_never().unwrap().unwrap();
    assert_eq!(*trace.lock().unwrap(), ["request dropped"]);
    drop(running);
    assert_eq!(
        *trace.lock().unwrap(),
        ["request dropped", "driver stopped", "app exited"]
    );
}

#[tokio::test]
async fn quitting_at_start_exits_without_drawing_or_polling_input() {
    let (mut app, _) = build_app(false);
    let terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    EventLoop::builder()
        .components(app.screens.clone())
        .overlay(Overlay::new().component(Id::Page))
        .keymap(keys())
        .driver(app.jobs)
        .terminal(terminal)
        .input(stream::poll_fn(|_| panic!("input polled after quit")))
        .hooks(
            Hooks::default()
                .on_start(Box::new(|_, _, effects| effects.quit()))
                .on_quit(Box::new(|_, _| app.quits += 1))
                .on_exit(Box::new(|_, _| app.exits += 1)),
        )
        .build()
        .run()
        .await
        .unwrap();
    assert!(app.screens.page.trace.borrow().is_empty());
    assert_eq!((app.quits, app.exits), (1, 1));
}

#[tokio::test]
async fn idle_input_wakes_the_application_after_the_first_poll() {
    use std::{
        cell::Cell,
        future::Future as _,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Wake, Waker},
    };

    #[derive(Default)]
    struct Notification(AtomicBool);
    impl Wake for Notification {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
    }

    // Crossterm's event reader saves the first pending poll's waker until input arrives.
    let reader = RefCell::new(None);
    let ready = Cell::new(false);
    let mut input = stream::poll_fn(|cx| {
        if ready.get() {
            return Poll::Ready(Some(Ok(Input::FocusGained)));
        }
        reader
            .borrow_mut()
            .get_or_insert_with(|| cx.waker().clone());
        Poll::Pending
    });
    let (mut app, _) = build_app(false);
    let terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut events = app.event_loop(terminal, &mut input).start();
    let signal = Arc::new(Notification::default());
    let waker = Waker::from(Arc::clone(&signal));
    let mut cx = Context::from_waker(&waker);
    let mut step = Box::pin(events.process_next());
    assert!(step.as_mut().poll(&mut cx).is_pending());
    signal.0.store(false, Ordering::Release);
    ready.set(true);
    reader.borrow().as_ref().unwrap().wake_by_ref();
    assert!(signal.0.load(Ordering::Acquire));
    assert!(matches!(step.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}
