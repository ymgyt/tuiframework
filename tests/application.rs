use std::{cell::RefCell, future::pending, io, rc::Rc};

use futures_util::{FutureExt as _, stream};
use tokio::sync::oneshot;
use tuiframework::{
    AppKeymap, Application, Component, Components, Driver, DriverContext, Effects, KeyAction,
    RenderContext, RunError,
    crossterm::event::Event as Input,
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

struct Pane {
    id: Id,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl Component<App> for Pane {
    fn key_layers(&self) -> &[()] {
        &[()]
    }

    fn render(&self, frame: &mut Frame<'_>, _: &RenderContext<'_, App>) {
        frame.render_widget(Paragraph::new("visible"), frame.area());
        self.trace.borrow_mut().push("render");
    }

    fn subscribes(&self, event: &Event) -> bool {
        matches!(self.id, Id::Observer) || matches!(event, Event::Changed)
    }

    fn on_event(&mut self, event: &Event) -> Effects<App> {
        match (self.id, event) {
            (Id::Page, Event::Changed) => {
                self.trace.borrow_mut().push("page changed");
                Effects::NONE
                    .emit(Event::FollowUp)
                    .send(Id::Observer, Command)
            }
            (Id::Observer, Event::FollowUp) => {
                self.trace.borrow_mut().push("observer follow-up");
                Effects::NONE
            }
            (Id::Observer, Event::Changed) => {
                self.trace.borrow_mut().push("observer changed");
                Effects::NONE
            }
            _ => unreachable!(),
        }
    }

    fn command(&mut self, _: Command) -> Effects<App> {
        assert!(matches!(self.id, Id::Observer));
        self.trace.borrow_mut().push("observer command");
        Effects::NONE
    }
}

struct App {
    page: Pane,
    observer: Pane,
    animating: bool,
}

impl Components for App {
    type Id = Id;
    type Layer = ();
    type Command = Command;
    type Theme = ();
    fn theme(&self) -> &() {
        &()
    }
    type Keymap = AppKeymap<Self, NotifyObserver>;
    type Event = Event;
    type Operation = Hold;
    type View<'a> = RenderContext<'a, Self>;
    fn ids(&self) -> impl IntoIterator<Item = Id> {
        [Id::Page, Id::Observer]
    }
    fn component(&self, id: Id) -> Option<&dyn Component<Self>> {
        Some(match id {
            Id::Page => &self.page,
            Id::Observer => &self.observer,
        })
    }
    fn component_mut(&mut self, id: Id) -> Option<&mut dyn Component<Self>> {
        Some(match id {
            Id::Page => &mut self.page,
            Id::Observer => &mut self.observer,
        })
    }
    fn visible(&self) -> impl IntoIterator<Item = Id> {
        [Id::Page]
    }
    fn is_busy(&self) -> bool {
        self.animating
    }
    fn tick(&mut self) {
        self.page.trace.borrow_mut().push("tick");
    }
    fn start(&mut self) -> Effects<Self> {
        Effects::NONE.emit(Event::Changed).operation(Hold)
    }
}

struct Jobs {
    sender: Option<oneshot::Sender<()>>,
}
impl Driver<App> for Jobs {
    fn dispatch(&mut self, _: Hold, cx: &mut DriverContext<Event>) {
        let guard = self.sender.take().unwrap();
        cx.submit(async move {
            let _held = guard;
            pending().await
        });
    }
}

fn build_app(animating: bool) -> (Application<App, Jobs>, oneshot::Receiver<()>) {
    let (sender, receiver) = oneshot::channel();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let app = App {
        animating,
        page: Pane {
            id: Id::Page,
            trace: Rc::clone(&trace),
        },
        observer: Pane {
            id: Id::Observer,
            trace,
        },
    };
    let mut keys = KeymapConfig::default();
    keys.bind(
        (),
        Binding {
            on: KeySequence::parse(["x"]).unwrap(),
            command: NotifyObserver,
            desc: None,
        },
    );
    (
        Application::builder()
            .components(app)
            .driver(Jobs {
                sender: Some(sender),
            })
            .keymap(AppKeymap::<App, NotifyObserver>::compile(keys).unwrap())
            .tick_interval(std::time::Duration::from_secs(2))
            .build(),
        receiver,
    )
}

#[tokio::test]
async fn a_key_action_can_address_another_component_with_a_non_clone_command() {
    let (mut app, _) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut input = stream::iter([Ok(Input::Key(KeyStroke::from_char('x').into()))]);
    app.run_with(&mut terminal, &mut input).await.unwrap();
    assert!(app.components().page.trace.borrow().ends_with(&[
        "render",
        "observer command",
        "render"
    ]));
}

#[tokio::test]
async fn hidden_subscribers_observe_facts_before_commands_and_eof_cancels_work() {
    let (mut application, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    application
        .run_with(&mut terminal, &mut stream::empty())
        .await
        .unwrap();
    assert_eq!(
        *application.components().page.trace.borrow(),
        [
            "page changed",
            "observer follow-up",
            "observer changed",
            "observer command",
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
        let mut run = Box::pin(application.run_with(&mut terminal, &mut input));
        assert!(run.as_mut().now_or_never().is_none());
        assert_eq!(dropped.try_recv(), Err(oneshot::error::TryRecvError::Empty));
    }
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );

    let (mut application, mut dropped) = build_app(false);
    let mut input = stream::iter([Err(io::Error::other("input failed"))]);
    let error = application
        .run_with(&mut terminal, &mut input)
        .await
        .unwrap_err();
    assert!(matches!(error, RunError::Input(e) if e.to_string() == "input failed"));
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[test]
fn synchronous_render_starts_once_and_dropping_the_app_releases_the_job() {
    let (mut app, mut dropped) = build_app(false);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    app.render(&mut terminal).unwrap();
    app.render(&mut terminal).unwrap();
    assert!(app.has_pending_work());
    drop(app);
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[tokio::test(start_paused = true)]
async fn only_busy_components_wake_the_loop_on_the_configured_interval() {
    use std::time::Duration;

    let (mut idle, _) = build_app(false);
    let (mut busy, _) = build_app(true);
    let period = Duration::from_secs(2);
    let mut input = stream::pending();
    busy.step(&mut input).await.unwrap();
    tokio::time::advance(period / 2).await;
    assert!(busy.step(&mut input).now_or_never().is_none());
    tokio::time::advance(period / 2).await;
    busy.step(&mut input).await.unwrap();
    assert!(idle.step(&mut input).now_or_never().is_none());
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
    app.send(Id::Observer, Command);
    let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
    drop(app.run_with(&mut terminal, &mut stream::pending()));
    assert!(app.is_finished());
    assert!(!app.has_pending_work());
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
}

#[tokio::test]
async fn terminal_setup_failure_stops_prestarted_work() {
    use std::io::IsTerminal as _;
    if std::io::stdout().is_terminal() {
        return;
    }
    let (mut app, mut dropped) = build_app(false);
    app.send(Id::Observer, Command);
    assert!(matches!(app.run().await, Err(RunError::Setup(_))));
    assert!(app.is_finished());
    assert_eq!(
        dropped.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
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
    let signal = Arc::new(Notification::default());
    let waker = Waker::from(Arc::clone(&signal));
    let mut cx = Context::from_waker(&waker);
    let mut step = Box::pin(app.step(&mut input));
    assert!(step.as_mut().poll(&mut cx).is_pending());
    signal.0.store(false, Ordering::Release);
    ready.set(true);
    reader.borrow().as_ref().unwrap().wake_by_ref();
    assert!(signal.0.load(Ordering::Acquire));
    assert!(matches!(step.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}
