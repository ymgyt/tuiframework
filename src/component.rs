use std::hash::Hash;

use crossterm::event::Event as TerminalEvent;
use ratatui::{Frame, layout::Rect};

use crate::keymap::{KeyHint, Keymap, LayerStack};

/// Keymap built from the command, layer, and component types of one application.
pub type AppKeymap<A> =
    Keymap<<A as Components>::Layer, <A as Components>::Binding, <A as Components>::Id>;

/// Resolves a binding to its destination and command once the active layer's
/// owner is known. Binding a command directly sends it to that owner.
pub trait KeyAction<A: Components> {
    fn command_for(self, owner: A::Id) -> (A::Id, A::Command);
}

impl<A: Components> KeyAction<A> for A::Command {
    fn command_for(self, owner: A::Id) -> (A::Id, A::Command) {
        (owner, self)
    }
}

/// The app's vocabulary and concrete component store. An ID may address a
/// nested field of another component.
pub trait Components: Sized {
    type Id: Copy + Eq;
    type Layer: Copy + Eq + Hash;
    type Command;
    type Binding: Clone + KeyAction<Self>;
    type Event;
    type Operation;
    /// Data shared by every component in one draw. Use `RenderContext<'a, Self>`
    /// directly or convert it into a view with a shared, app-specific layout.
    type View<'a>: From<RenderContext<'a, Self>>
    where
        Self: 'a;

    /// Every component ID once, in event delivery order, including hidden components.
    fn ids() -> impl IntoIterator<Item = Self::Id>;

    fn component(&self, id: Self::Id) -> &dyn Component<Self>;
    fn component_mut(&mut self, id: Self::Id) -> &mut dyn Component<Self>;

    /// Handles an addressed command. Override this to route a domain command
    /// using sibling state, for example a filter command for the selected tab.
    fn command(&mut self, to: Self::Id, command: Self::Command) -> Effects<Self> {
        self.component_mut(to).command(command)
    }

    /// Visible component IDs, from the back of the screen to the front.
    fn visible(&self) -> impl IntoIterator<Item = Self::Id>;

    fn start(&mut self) -> Effects<Self> {
        Effects::NONE
    }

    /// Observes terminal input before its bound command is delivered. The
    /// binding is resolved against the layers of the frame the user saw.
    fn on_input(&mut self, _input: &TerminalEvent) -> Effects<Self> {
        Effects::NONE
    }

    /// Install or reset runtime layers here when their backing data changes.
    fn update_keymap(&mut self, _keymap: &mut AppKeymap<Self>) {}

    /// Enables timer wakeups while an animation is active.
    fn is_busy(&self) -> bool {
        false
    }

    fn tick(&mut self) {}
}

/// One addressable part of an application. State changes stay with its owner.
pub trait Component<A: Components> {
    fn render(&self, frame: &mut Frame<'_>, cx: &A::View<'_>);

    fn key_layers(&self) -> &[A::Layer] {
        &[]
    }

    fn mask_lower_layers(&self, _layers: &mut LayerStack<A::Layer, A::Id>) {}

    fn command(&mut self, _command: A::Command) -> Effects<A> {
        Effects::NONE
    }

    /// Checked in registration order for each published event, including while hidden.
    fn subscribes(&self, _event: &A::Event) -> bool {
        false
    }

    fn on_event(&mut self, _event: &A::Event) -> Effects<A> {
        Effects::NONE
    }
}

/// The shared state and resolved input layers for the frame being drawn.
pub struct RenderContext<'a, A: Components> {
    pub components: &'a A,
    pub area: Rect,
    pub focused: bool,
    pub keymap: &'a AppKeymap<A>,
    pub layers: &'a LayerStack<A::Layer, A::Id>,
}

impl<A: Components> RenderContext<'_, A> {
    pub fn hints(&self) -> Vec<KeyHint<'_, A::Layer, A::Binding>> {
        self.keymap.hints(self.layers)
    }
}

/// Facts, addressed commands, and external operations produced by one state transition.
#[must_use]
pub struct Effects<A: Components> {
    pub(crate) events: Vec<A::Event>,
    pub(crate) commands: Vec<(A::Id, A::Command)>,
    pub(crate) operations: Vec<A::Operation>,
    pub(crate) quit: bool,
}

impl<A: Components> Effects<A> {
    pub const NONE: Self = Self {
        events: Vec::new(),
        commands: Vec::new(),
        operations: Vec::new(),
        quit: false,
    };

    /// Publishes a fact to all interested components.
    pub fn emit(mut self, event: A::Event) -> Self {
        self.events.push(event);
        self
    }

    /// Addresses a follow-up command to a component, including one currently hidden.
    pub fn send(mut self, to: A::Id, command: A::Command) -> Self {
        self.commands.push((to, command));
        self
    }

    /// Requests work from the application's driver.
    pub fn operation(mut self, operation: A::Operation) -> Self {
        self.operations.push(operation);
        self
    }

    pub fn quit(mut self) -> Self {
        self.quit = true;
        self
    }
}
