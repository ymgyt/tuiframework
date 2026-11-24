use futures_util::stream;
use std::cell::Cell;
use tuiframework::{
    crossterm::event::{Event as Input, KeyEvent, KeyEventKind},
    keymap::KeyStroke,
    ratatui::{Terminal, backend::TestBackend},
};

use super::*;

struct Fixture<'a>(&'a Cell<usize>);

impl Driver<Files> for Fixture<'_> {
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

fn application<D: Driver<Files>>(driver: D) -> Application<Files, D> {
    Application::builder()
        .components(Files::new(".".into(), Palette::ferra().into()))
        .driver(driver)
        .keymap(keymap().unwrap())
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
    let mut app = application(Fixture(&reads));
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    app.render(&mut terminal).unwrap();
    while app.has_pending_work() {
        app.step(&mut stream::pending()).await.unwrap();
    }
    app.run_with(&mut terminal, &mut stream::iter(keys.into_iter().map(Ok)))
        .await
        .unwrap();
    (rows(&terminal).join("\n"), reads.get())
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

impl Driver<Files> for PendingRead {
    fn dispatch(&mut self, _: Operation, _: &mut DriverContext<Event>) {}
}

#[test]
fn moving_back_up_keeps_the_viewport_and_resizing_keeps_the_selection() {
    let mut app = application(PendingRead);
    let mut terminal = Terminal::new(TestBackend::new(24, 5)).unwrap();
    app.publish(&Event::Loaded(Ok(["alpha", "bravo", "charlie", "delta"]
        .map(str::to_owned)
        .into())));
    for _ in 0..3 {
        app.send(Id::Files, Command::Next);
        app.render(&mut terminal).unwrap();
    }
    assert_eq!(&rows(&terminal)[1..4], ["  bravo", "  charlie", "> delta"]);

    app.send(Id::Files, Command::Previous);
    app.render(&mut terminal).unwrap();
    assert_eq!(&rows(&terminal)[1..4], ["  bravo", "> charlie", "  delta"]);

    terminal.backend_mut().resize(24, 3);
    app.render(&mut terminal).unwrap();
    assert_eq!(rows(&terminal)[1], "> charlie");
}

#[tokio::test]
async fn switching_theme_restyles_a_directory_read_in_progress() {
    let mut app = application(PendingRead);
    let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
    app.render(&mut terminal).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, 0)].fg,
        Palette::ferra().accent
    );

    let mut input = stream::iter([Ok(key("t")), Ok(key("q"))]);
    app.run_with(&mut terminal, &mut input).await.unwrap();
    assert!(rows(&terminal)[0].contains("loading"));
    let header = &terminal.backend().buffer()[(0, 0)];
    assert_eq!(header.fg, Palette::dracula().accent);
    assert_eq!(header.bg, Palette::dracula().background);
}
