use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::Frame;

use crate::{
    component::{AppKeymap, Components, Effects, KeyAction as _, RenderContext},
    keymap::{KeyStroke, LayerStack, Resolution},
};

/// Routes inputs and facts to the app's components and draws the visible stack.
pub struct Compositor<A: Components> {
    pub components: A,
    keymap: AppKeymap<A>,
    visible: Vec<A::Id>,
    layers: LayerStack<A::Layer, A::Id>,
    focused: bool,
    pub(crate) quit: bool,
}

impl<A: Components> Compositor<A> {
    pub fn new(components: A, keymap: AppKeymap<A>) -> Self {
        let mut compositor = Self {
            components,
            keymap,
            visible: Vec::new(),
            layers: LayerStack::default(),
            focused: true,
            quit: false,
        };
        compositor.refresh();
        compositor
    }

    pub(crate) fn start(&mut self) -> Vec<A::Operation> {
        self.quit = false;
        self.focused = true;
        self.keymap.cancel_pending();
        let effects = self.components.start();
        self.apply(effects)
    }

    /// Rebuilds the display order and layers after direct changes to app state.
    /// Input and event dispatch already do this after applying their effects.
    pub fn refresh(&mut self) {
        self.components.update_keymap(&mut self.keymap);
        self.visible.clear();
        self.visible.extend(self.components.visible());
        self.layers.clear();
        for &id in &self.visible {
            let component = self.components.component(id);
            component.mask_lower_layers(&mut self.layers);
            self.layers.extend(id, component.key_layers());
        }
        self.keymap.sync_layers(&self.layers);
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let cx = RenderContext {
            components: &self.components,
            area: frame.area(),
            focused: self.focused,
            keymap: &self.keymap,
            layers: &self.layers,
        };
        let view = A::View::from(cx);
        for &id in &self.visible {
            self.components.component(id).render(frame, &view);
        }
    }

    pub fn resolve_key(&mut self, key: KeyStroke) -> Option<(A::Id, A::Command)> {
        match self.keymap.resolve(&self.layers, key) {
            Resolution::Matched { owner, command } => Some(command.command_for(owner)),
            _ => None,
        }
    }

    pub fn input(&mut self, input: &TerminalEvent) -> Vec<A::Operation> {
        if matches!(input, TerminalEvent::Key(key) if key.kind == KeyEventKind::Release) {
            return Vec::new();
        }
        let mut effects = self.components.on_input(input);
        match input {
            TerminalEvent::FocusLost => {
                self.focused = false;
                self.keymap.cancel_pending();
            }
            TerminalEvent::FocusGained => self.focused = true,
            TerminalEvent::Key(key) => {
                if let Some((owner, command)) = self.resolve_key((*key).into()) {
                    effects = effects.send(owner, command);
                }
            }
            _ => {}
        }
        self.apply(effects)
    }

    pub fn event(&mut self, event: &A::Event) -> Vec<A::Operation> {
        let mut operations = Vec::new();
        self.broadcast(std::slice::from_ref(event), &mut operations);
        self.refresh();
        operations
    }

    fn apply(&mut self, effects: Effects<A>) -> Vec<A::Operation> {
        let mut operations = Vec::new();
        self.apply_into(effects, &mut operations);
        self.refresh();
        operations
    }

    fn apply_into(&mut self, effects: Effects<A>, operations: &mut Vec<A::Operation>) {
        self.broadcast(&effects.events, operations);
        self.run_reply(effects, operations);
    }

    fn broadcast(&mut self, events: &[A::Event], operations: &mut Vec<A::Operation>) {
        if !events.is_empty() {
            let mut replies = Vec::new();
            self.notify(events, &mut replies);
            for reply in replies {
                self.run_reply(reply, operations);
            }
        }
    }

    fn run_reply(&mut self, reply: Effects<A>, operations: &mut Vec<A::Operation>) {
        self.quit |= reply.quit;
        operations.extend(reply.operations);
        for (to, command) in reply.commands {
            let next = self.components.command(to, command);
            self.apply_into(next, operations);
        }
    }

    /// Nested facts reach their subscribers before follow-up commands run.
    fn notify(&mut self, events: &[A::Event], replies: &mut Vec<Effects<A>>) {
        for event in events {
            for id in A::ids() {
                if self.components.component(id).subscribes(event) {
                    let mut reply = self.components.component_mut(id).on_event(event);
                    self.notify(&reply.events, replies);
                    reply.events.clear();
                    replies.push(reply);
                }
            }
        }
    }
}
