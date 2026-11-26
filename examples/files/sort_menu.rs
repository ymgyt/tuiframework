use tuiframework::{
    Component, Effects, RenderContext,
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
    pub(super) open: bool,
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

    fn apply_command(&mut self, command: Command) -> Effects<Files> {
        match command {
            Command::OpenSort => {
                self.selected = self.applied;
                self.open = true;
            }
            Command::Next => self.selected = 1,
            Command::Previous | Command::First => self.selected = 0,
            Command::Close => self.open = false,
            Command::Choose => {
                self.open = false;
                self.applied = self.selected;
                let order = if self.selected == 0 {
                    Order::Ascending
                } else {
                    Order::Descending
                };
                return Effects::NONE.emit(Event::Sorted(order));
            }
            _ => {}
        }
        Effects::NONE
    }

    fn render(&self, frame: &mut Frame<'_>, cx: &RenderContext<'_, Files>) {
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
                    .style(cx.theme.common.base),
            )
            .highlight_symbol("> ")
            .highlight_style(cx.theme.common.selected);
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(
            list,
            area,
            &mut ListState::default().with_selected(Some(self.selected)),
        );
    }
}
