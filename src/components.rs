use crate::{Component, RenderInputs, keymap::KeyBindings};
use std::hash::Hash;

/// Application-owned components and the types they use to communicate.
pub trait Components: Sized {
    type Id: Copy + Eq;
    type OverlayId: Copy + Eq;
    type StackId: Copy + Eq;
    type Layer: Copy + Eq + Hash;
    type Command;
    type Keymap: KeyBindings<Self>;
    type Event;
    type Operation;
    type RenderContext<'a>: From<RenderInputs<'a, Self>>
    where
        Self: 'a;

    /// All registered IDs in event delivery order, including hidden components.
    fn ids(&self) -> impl IntoIterator<Item = Self::Id>;
    fn get(&self, id: Self::Id) -> Option<&dyn Component<Self>>;
    fn get_mut(&mut self, id: Self::Id) -> Option<&mut dyn Component<Self>>;
}
