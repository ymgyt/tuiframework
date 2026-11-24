use futures_util::{StreamExt as _, stream};
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

async fn screen(keys: Vec<Input>) -> (String, usize) {
    let reads = Cell::new(0);
    let mut application =
        Application::new(Files::new(".".into()), Fixture(&reads), keymap().unwrap());
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    // The first input arrives after the queued directory result.
    let mut input = Box::pin(
        stream::once(async {
            tokio::task::yield_now().await;
            Ok(Input::FocusGained)
        })
        .chain(stream::iter(keys.into_iter().map(Ok))),
    );
    application
        .run_with(&mut terminal, &mut input)
        .await
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer
        .content()
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(tuiframework::ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    (rows.join("\n"), reads.get())
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
