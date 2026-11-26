use ratatui::{Frame, layout::Rect};

use crate::{TerminalFocus, Ui, keymap::LayerStack};

/// One addressable part of an application. State changes stay with its owner.
pub trait Component<U: Ui> {
    fn render(&self, frame: &mut Frame<'_>, cx: &U::Context<'_>);

    fn key_layers(&self) -> &[U::Layer] {
        &[]
    }

    fn mask_lower_layers(&self, _layers: &mut LayerStack<U::Layer, U::Id>) {}

    fn apply_command(&mut self, _command: U::Command) -> Effects<U> {
        Effects::NONE
    }

    /// Checked in registration order for each published event, including while hidden.
    fn subscribes(&self, _event: &U::Event) -> bool {
        false
    }

    fn apply_event(&mut self, _event: &U::Event) -> Effects<U> {
        Effects::NONE
    }
}

/// The shared state and resolved input layers for the frame being drawn.
pub struct RenderContext<'a, U: Ui> {
    pub ui: &'a U,
    pub theme: &'a U::Theme,
    pub area: Rect,
    pub focus: TerminalFocus,
    pub keymap: &'a U::Keymap,
    pub layers: &'a LayerStack<U::Layer, U::Id>,
}

/// Facts, addressed commands, and external operations produced by one state transition.
///
/// Events reach subscribers in registration order. Each subscriber's nested
/// events reach their subscribers before the next subscriber receives the
/// original event. Once all those notifications finish, their replies run in
/// that same depth-first completion order, followed by the originating reply.
/// Each reply contributes its operations and runs its commands in order. The
/// driver receives the collected operations after all commands have run.
#[must_use]
pub struct Effects<U: Ui> {
    pub(crate) events: Vec<U::Event>,
    pub(crate) commands: Vec<(U::Id, U::Command)>,
    pub(crate) operations: Vec<U::Operation>,
    pub(crate) quit: bool,
}

impl<U: Ui> Effects<U> {
    pub const NONE: Self = Self {
        events: Vec::new(),
        commands: Vec::new(),
        operations: Vec::new(),
        quit: false,
    };

    /// Publishes a fact to all interested components.
    pub fn emit(mut self, event: U::Event) -> Self {
        self.events.push(event);
        self
    }

    /// Addresses a follow-up command to a component, including one currently hidden.
    pub fn send(mut self, to: U::Id, command: U::Command) -> Self {
        self.commands.push((to, command));
        self
    }

    /// Delivers a batch to the same component in order.
    pub fn send_all(mut self, to: U::Id, commands: impl IntoIterator<Item = U::Command>) -> Self {
        self.commands
            .extend(commands.into_iter().map(|command| (to, command)));
        self
    }

    /// Requests work from the application's driver.
    pub fn operation(mut self, operation: U::Operation) -> Self {
        self.operations.push(operation);
        self
    }

    /// Collects an ordered batch of work produced by one transition.
    pub fn from_operations(operations: impl IntoIterator<Item = U::Operation>) -> Self {
        Self {
            operations: operations.into_iter().collect(),
            ..Self::NONE
        }
    }

    /// Addressed commands in delivery order.
    pub fn commands(&self) -> &[(U::Id, U::Command)] {
        &self.commands
    }

    /// External operations in dispatch order.
    pub fn operations(&self) -> &[U::Operation] {
        &self.operations
    }

    pub fn quit(mut self) -> Self {
        self.quit = true;
        self
    }
}
