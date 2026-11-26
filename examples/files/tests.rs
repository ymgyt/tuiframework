use futures_util::{StreamExt as _, stream};
use std::cell::Cell;
use tuiframework::{
    crossterm::event::{Event as Input, KeyEvent, KeyEventKind},
    event_loop::Ready,
    keymap::KeyStroke,
    ratatui::{Terminal, backend::TestBackend},
};

use super::*;

struct Fixture<'a>(&'a Cell<usize>);

impl Driver for Fixture<'_> {
    type Event = Event;
    type Operation = Operation;
    fn dispatch(&mut self, _: Operation, cx: &mut DriverContext<Event>) {
        self.0.set(self.0.get() + 1);
        cx.emit(Event::Loaded(Ok(["alpha", "bravo", "charlie"]
            .map(str::to_owned)
            .into())));
    }
}

fn key(notation: &str) -> Input {
    Input::Key(notation.parse::<KeyStroke>().unwrap().into())
}

fn application<D, T, I>(
    driver: D,
    terminal: T,
    input: I,
) -> EventLoop<Ready<'static, Files, D>, T, I>
where
    D: Driver<Event = Event, Operation = Operation>,
    T: tuiframework::RenderTarget,
    I: futures_util::Stream<Item = io::Result<Input>> + Unpin,
{
    let files = Files::new(".".into(), Palette::ferra().into());
    let hooks = Hooks::default().on_start(Box::new(|_, _| {
        Effects::NONE.send(Id::Files, Command::Reload)
    }));
    EventLoop::builder()
        .compositor(Compositor::new(files, keymap().unwrap()))
        .driver(driver)
        .terminal(terminal)
        .input(input)
        .hooks(hooks)
        .build()
}

fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| {
            row.iter()
                .map(tuiframework::ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

async fn screen(keys: Vec<Input>) -> (String, usize) {
    let reads = Cell::new(0);
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let input = stream::poll_fn(move |cx| receiver.poll_recv(cx));
    let mut app = application(Fixture(&reads), &mut terminal, input).start();
    while app.has_pending_work() {
        app.process_next().await.unwrap();
    }
    for key in keys {
        sender.send(Ok(key)).unwrap();
    }
    drop(sender);
    app.run().await.unwrap();
    (rows(&terminal).join("\n"), reads.get())
}

#[tokio::test]
async fn queued_reload_keys_are_handled_before_a_completed_directory_read() {
    const KEY_COUNT: usize = 256;
    let reads = Cell::new(0);
    let terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
    let input = stream::iter((0..KEY_COUNT).map(|_| Ok(key("r")))).chain(stream::pending());
    let mut app = application(Fixture(&reads), terminal, input).start();

    for _ in 0..KEY_COUNT {
        app.process_next().await.unwrap();
    }
    assert!(app.ui().is_busy());
    app.process_next().await.unwrap();
    assert!(!app.ui().is_busy());
    assert_eq!(reads.get(), 1);
}

#[tokio::test]
async fn menu_owns_shared_movement_and_masks_background_reload() {
    let (screen, reads) = screen(vec![key("j"), key("s"), key("j"), key("r")]).await;
    assert_eq!(reads, 1);
    assert!(screen.contains("> bravo"));
    assert!(screen.contains("> Name descending"));
    assert!(!screen.contains("r reload"));
}

#[tokio::test]
async fn applying_sort_updates_the_base_screen_and_restores_its_bindings() {
    let mut release = KeyEvent::from("j".parse::<KeyStroke>().unwrap());
    release.kind = KeyEventKind::Release;
    let (screen, _) = screen(vec![
        key("s"),
        key("j"),
        key("enter"),
        Input::Key(release),
        key("q"),
    ])
    .await;
    assert!(screen.contains("> charlie"));
    assert!(screen.contains("r reload"));
    assert!(!screen.contains("Sort files"));
}

/// The listing stays in its loading state until the test supplies a result.
struct PendingRead;

impl Driver for PendingRead {
    type Event = Event;
    type Operation = Operation;
    fn dispatch(&mut self, _: Operation, _: &mut DriverContext<Event>) {}
}

#[test]
fn moving_back_up_keeps_the_viewport_and_resizing_keeps_the_selection() {
    let terminal = Terminal::new(TestBackend::new(24, 5)).unwrap();
    let mut app = application(PendingRead, terminal, stream::pending()).start();
    let names = ["alpha", "bravo", "charlie", "delta"].map(str::to_owned);
    app.apply_effects(Effects::NONE.emit(Event::Loaded(Ok(names.into()))));
    for _ in 0..3 {
        app.apply_effects(Effects::NONE.send(Id::Files, Command::Next));
        app.render().unwrap();
    }
    assert_eq!(
        &rows(app.terminal())[1..4],
        ["  bravo", "  charlie", "> delta"]
    );

    app.apply_effects(Effects::NONE.send(Id::Files, Command::Previous));
    app.render().unwrap();
    assert_eq!(
        &rows(app.terminal())[1..4],
        ["  bravo", "> charlie", "  delta"]
    );

    app.terminal_mut().backend_mut().resize(24, 3);
    app.render().unwrap();
    assert_eq!(rows(app.terminal())[1], "> charlie");
}

#[tokio::test]
async fn switching_theme_restyles_a_directory_read_in_progress() {
    let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
    let input = stream::iter([Ok(key("t")), Ok(key("q"))]);
    let mut app = application(PendingRead, &mut terminal, input).start();
    app.render().unwrap();
    assert_eq!(
        app.terminal().backend().buffer()[(0, 0)].fg,
        Palette::ferra().accent
    );

    app.run().await.unwrap();
    assert!(rows(&terminal)[0].contains("loading"));
    let header = &terminal.backend().buffer()[(0, 0)];
    assert_eq!(header.fg, Palette::dracula().accent);
    assert_eq!(header.bg, Palette::dracula().background);
}
