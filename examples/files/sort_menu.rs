use tuiframework::{
    Component, Effects, RenderInputs,
    keymap::LayerStack,
    ratatui::{
        Frame,
        layout::{Constraint, Flex, Layout},
        widgets::{Block, Clear, List, ListState},
    },
};

use super::{Command, Event, Files, Id, Layer};

#[derive(Clone, Copy, Default)]
pub(super) enum Order {
    #[default]
    Ascending,
    Descending,
}

#[derive(Default)]
pub(super) struct SortMenu {
    selected: usize,
    applied: usize,
}

impl Component<Files> for SortMenu {
    fn key_layers(&self) -> &[Layer] {
        &[Layer::List, Layer::Sort]
    }

    fn mask_lower_layers(&self, layers: &mut LayerStack<Layer, Id>) {
        layers.cover(&[Layer::Files]);
    }

    fn subscribes(&self, event: &Event) -> bool {
        matches!(event, Event::SortRequested)
    }

    fn apply_event(&mut self, _: &Event, effects: &mut Effects<Files>) {
        self.selected = self.applied;
        effects.open(());
    }

    fn apply_command(&mut self, command: Command, effects: &mut Effects<Files>) {
        match command {
            Command::Next => self.selected = 1,
            Command::Previous | Command::First => self.selected = 0,
            Command::Close => effects.close(),
            Command::Choose => {
                effects.close();
                self.applied = self.selected;
                let order = if self.selected == 0 {
                    Order::Ascending
                } else {
                    Order::Descending
                };
                effects.emit(Event::Sorted(order));
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame<'_>, cx: &RenderInputs<'_, Files>) {
        let [row] = Layout::vertical([Constraint::Length(4)])
            .flex(Flex::Center)
            .areas(frame.area());
        let [area] = Layout::horizontal([Constraint::Length(26)])
            .flex(Flex::Center)
            .areas(row);
        let list = List::new(["Name ascending", "Name descending"])
            .block(
                Block::bordered()
                    .title("Sort files")
                    .style(cx.components.listing.theme.common.base),
            )
            .highlight_symbol("> ")
            .highlight_style(cx.components.listing.theme.common.selected);
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(
            list,
            area,
            &mut ListState::default().with_selected(Some(self.selected)),
        );
    }
}
