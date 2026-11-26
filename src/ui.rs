use std::hash::Hash;

use crossterm::event;

use crate::{Component, Effects, RenderContext, keymap::KeyBindings};

/// Application-specific UI state and behavior shared across its components.
/// Defines their messages, routes commands, and supplies the render context and display order.
pub trait Ui: Sized {
    /// A stable address. Assign a fresh ID if a removed
    /// component's queued commands could outlive it.
    type Id: Copy + Eq;
    type Layer: Copy + Eq + Hash;
    type Command;
    /// The compiled keymap shared by input dispatch and the render context.
    type Keymap: KeyBindings<Self>;
    type Event;
    type Operation;
    /// The application owns its current theme and may change it between frames.
    type Theme: ?Sized;
    /// Data shared by every component in one draw. Use `RenderContext<'a, Self>`
    /// directly or convert it into an application-specific context.
    type Context<'a>: From<RenderContext<'a, Self>>
    where
        Self: 'a;

    fn theme(&self) -> &Self::Theme;

    /// Every currently registered ID once, in event delivery order, including
    /// hidden components. Each published event takes its own snapshot.
    fn ids(&self) -> impl IntoIterator<Item = Self::Id>;

    /// Looks up a component by ID. Returns `None` if it has closed.
    fn component(&self, id: Self::Id) -> Option<&dyn Component<Self>>;
    fn component_mut(&mut self, id: Self::Id) -> Option<&mut dyn Component<Self>>;

    /// Handles an addressed command. Override this to route a domain command
    /// using sibling state, for example a filter command for the selected tab.
    fn apply_command(&mut self, to: Self::Id, command: Self::Command) -> Effects<Self> {
        self.component_mut(to)
            .map_or(Effects::NONE, |component| component.apply_command(command))
    }

    /// Visible component IDs, from the back of the screen to the front.
    fn visible(&self) -> impl IntoIterator<Item = Self::Id>;

    /// Observes terminal input before its bound command is delivered. The
    /// binding is resolved against the layers of the frame the user saw.
    fn on_input(&mut self, _input: &event::Event) -> Effects<Self> {
        Effects::NONE
    }

    /// Install or reset runtime layers here when their backing data changes.
    fn update_keymap(&mut self, _keymap: &mut Self::Keymap) {}

    /// Enables timer wakeups while an animation is active.
    fn is_busy(&self) -> bool {
        false
    }

    fn tick(&mut self) {}
}
