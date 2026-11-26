use std::{cell::Cell, path::PathBuf, sync::Arc};

use tuiframework::{
    Component, Effects, RenderContext,
    ratatui::{
        Frame,
        layout::{Constraint, Layout},
        widgets::{Block, List, ListState, Paragraph},
    },
};

use super::{Command, Event, Files, Id, Layer, Operation, Order};

/// Directory contents and selection for the base screen.
pub(super) struct Listing {
    path: PathBuf,
    names: Arc<[String]>,
    selected: usize,
    offset: Cell<usize>,
    order: Order,
    pub(super) loading: bool,
    tick: usize,
    error: Option<String>,
}

impl Listing {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            names: Arc::default(),
            selected: 0,
            offset: Cell::new(0),
            order: Order::default(),
            loading: false,
            tick: 0,
            error: None,
        }
    }

    const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

    pub(super) fn tick(&mut self) {
        self.tick = (self.tick + 1) % Self::SPINNER.len();
    }
}

impl Component<Files> for Listing {
    fn key_layers(&self) -> &[Layer] {
        &[Layer::Global, Layer::List, Layer::Files]
    }

    fn apply_command(&mut self, command: Command) -> Effects<Files> {
        match command {
            Command::Next => {
                self.selected = (self.selected + 1).min(self.names.len().saturating_sub(1));
            }
            Command::Previous => self.selected = self.selected.saturating_sub(1),
            Command::First => self.selected = 0,
            Command::Reload if !self.loading => {
                self.loading = true;
                self.error = None;
                return Effects::NONE.operation(Operation::ReadDirectory(self.path.clone()));
            }
            Command::OpenSort => return Effects::NONE.send(Id::Sort, command),
            Command::Quit => return Effects::NONE.quit(),
            _ => {}
        }
        Effects::NONE
    }

    fn subscribes(&self, _: &Event) -> bool {
        true
    }

    fn apply_event(&mut self, event: &Event) -> Effects<Files> {
        match event {
            Event::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(names) => {
                        self.names = Arc::clone(names);
                        self.selected = self.selected.min(names.len().saturating_sub(1));
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            Event::Sorted(order) => {
                self.order = *order;
                self.selected = 0;
            }
        }
        Effects::NONE
    }

    fn render(&self, frame: &mut Frame<'_>, cx: &RenderContext<'_, Files>) {
        let theme = &cx.theme.common;
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        frame.render_widget(Block::new().style(theme.base), frame.area());
        let path = self.path.display();
        let title = if self.loading {
            format!("{path}   {} loading", Self::SPINNER[self.tick])
        } else {
            format!("{path}   {} files", self.names.len())
        };
        let style = if self.loading {
            cx.theme.loading
        } else {
            theme.title
        };
        frame.render_widget(Paragraph::new(title).style(style), header);
        if let Some(error) = &self.error {
            frame.render_widget(Paragraph::new(error.as_str()).style(theme.error), body);
        } else {
            let names = (0..self.names.len()).map(|i| {
                let index = match self.order {
                    Order::Ascending => i,
                    Order::Descending => self.names.len() - i - 1,
                };
                self.names[index].as_str()
            });
            let list = List::new(names)
                .highlight_symbol("> ")
                .highlight_style(theme.selected);
            let mut state = ListState::default()
                .with_selected(Some(self.selected))
                .with_offset(self.offset.get());
            frame.render_stateful_widget(list, body, &mut state);
            self.offset.set(state.offset());
        }
        let hints = cx
            .keymap
            .hints(cx.layers)
            .into_iter()
            .filter_map(|h| h.description.map(|d| format!("{} {d}", h.keys)))
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(hints.join("  ")).style(theme.muted), footer);
    }
}
