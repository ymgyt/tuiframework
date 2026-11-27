use crossterm::event::Event as TerminalEvent;
use ratatui::{Frame, layout::Rect};

use crate::{Components, TerminalFocus, keymap::LayerStack, overlay::DisplayChange};

/// An addressable part of the application. Hidden components can still receive events.
pub trait Component<C: Components> {
    fn render(&self, frame: &mut Frame<'_>, cx: &C::RenderContext<'_>);
    fn key_layers(&self) -> &[C::Layer] {
        &[]
    }
    fn mask_lower_layers(&self, _layers: &mut LayerStack<C::Layer, C::Id>) {}
    fn update_keymap(&mut self, _keymap: &mut C::Keymap) {}
    fn apply_command(&mut self, _command: C::Command, _effects: &mut Effects<C>) {}
    fn subscribes(&self, _event: &C::Event) -> bool {
        false
    }
    fn apply_event(&mut self, _event: &C::Event, _effects: &mut Effects<C>) {}
    /// Observes accepted terminal input. Only the frontmost component receives `frontmost = true`.
    fn on_input(&mut self, _input: &TerminalEvent, _frontmost: bool, _effects: &mut Effects<C>) {}
    fn is_busy(&self) -> bool {
        false
    }
    fn tick(&mut self) {}
}

/// Shared input to the application's per-frame render context.
pub struct RenderInputs<'a, C: Components> {
    pub components: &'a C,
    pub area: Rect,
    pub focus: TerminalFocus,
    pub keymap: &'a C::Keymap,
    pub layers: &'a LayerStack<C::Layer, C::Id>,
}

/// Effects available at startup and to embedding applications.
pub struct ApplicationEffects<C: Components> {
    pub(crate) events: Vec<C::Event>,
    pub(crate) operations: Vec<C::Operation>,
    pub(crate) quit: bool,
}

impl<C: Components> Default for ApplicationEffects<C> {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            operations: Vec::new(),
            quit: false,
        }
    }
}

impl<C: Components> ApplicationEffects<C> {
    pub fn emit(&mut self, event: C::Event) {
        self.events.push(event);
    }
    pub fn operation(&mut self, operation: C::Operation) {
        self.operations.push(operation);
    }
    pub fn quit(&mut self) {
        self.quit = true;
    }
}

/// Effects of one component's input, command or event handler.
/// Display changes act on the component that issued them and are applied when its handler returns.
/// Published facts then reach subscribers in registration order, including hidden ones.
/// Nested facts complete before the next subscriber receives the parent fact. The driver
/// receives the accumulated external operations after the transition finishes.
pub struct Effects<C: Components> {
    pub(crate) app: ApplicationEffects<C>,
    pub(crate) display: Vec<DisplayChange<C>>,
}

impl<C: Components> Default for Effects<C> {
    fn default() -> Self {
        Self {
            app: ApplicationEffects::default(),
            display: Vec::new(),
        }
    }
}

impl<C: Components> Effects<C> {
    pub fn open(&mut self, overlay: C::OverlayId) {
        self.display.push(DisplayChange::Open(overlay));
    }
    pub fn push(&mut self, stack: C::StackId) {
        self.display.push(DisplayChange::Push(stack));
    }
    pub fn close(&mut self) {
        self.display.push(DisplayChange::Close);
    }
    pub fn emit(&mut self, event: C::Event) {
        self.app.emit(event);
    }
    pub fn operation(&mut self, operation: C::Operation) {
        self.app.operation(operation);
    }
    pub fn operations(&mut self, operations: impl IntoIterator<Item = C::Operation>) {
        self.app.operations.extend(operations);
    }
    pub fn quit(&mut self) {
        self.app.quit();
    }
}
