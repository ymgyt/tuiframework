use std::ops::ControlFlow;

use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::Frame;

use crate::{
    Ui,
    component::{Effects, RenderContext},
    keymap::{KeyBindings as _, KeyStroke, LayerStack},
};

/// Window focus reported by the terminal and passed to components when rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFocus {
    Gained,
    Lost,
}

/// External operations and loop control produced by a completed UI transition.
#[derive(Debug)]
#[must_use]
pub struct DispatchResult<O> {
    pub operations: Vec<O>,
    pub control_flow: ControlFlow<()>,
}

/// Routes inputs and facts to the app's components and draws the visible stack.
pub struct Compositor<U: Ui> {
    pub ui: U,
    keymap: U::Keymap,
    visible: Vec<U::Id>,
    layers: LayerStack<U::Layer, U::Id>,
    focus: TerminalFocus,
}

impl<U: Ui> Compositor<U> {
    pub fn new(ui: U, keymap: U::Keymap) -> Self {
        let mut compositor = Self {
            ui,
            keymap,
            visible: Vec::new(),
            layers: LayerStack::default(),
            focus: TerminalFocus::Gained,
        };
        compositor.refresh();
        compositor
    }

    /// Rebuilds the display order and layers after direct changes to app state.
    /// Input and event dispatch already do this after applying their effects.
    pub fn refresh(&mut self) {
        self.ui.update_keymap(&mut self.keymap);
        self.visible.clear();
        self.visible.extend(self.ui.visible());
        self.layers.clear();
        for &id in &self.visible {
            if let Some(component) = self.ui.component(id) {
                component.mask_lower_layers(&mut self.layers);
                self.layers.extend(id, component.key_layers());
            }
        }
        self.keymap.sync_layers(&self.layers);
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let cx = U::Context::from(RenderContext {
            ui: &self.ui,
            theme: self.ui.theme(),
            area: frame.area(),
            focus: self.focus,
            keymap: &self.keymap,
            layers: &self.layers,
        });
        for &id in &self.visible {
            if let Some(component) = self.ui.component(id) {
                component.render(frame, &cx);
            }
        }
    }

    /// Resolves a key against the current layers without applying its command.
    pub fn resolve_key(&mut self, key: KeyStroke) -> Option<(U::Id, U::Command)> {
        self.keymap.resolve_command(&self.layers, key)
    }

    pub(crate) fn tick(&mut self) {
        self.ui.tick();
        self.refresh();
    }

    pub fn apply_command(
        &mut self,
        to: U::Id,
        command: U::Command,
    ) -> DispatchResult<U::Operation> {
        self.dispatch(vec![Dispatch::Command(to, command)])
    }

    pub fn apply_terminal_event(&mut self, input: &TerminalEvent) -> DispatchResult<U::Operation> {
        if matches!(input, TerminalEvent::Key(key) if key.kind == KeyEventKind::Release) {
            return DispatchResult {
                operations: Vec::new(),
                control_flow: ControlFlow::Continue(()),
            };
        }
        let mut effects = self.ui.on_input(input);
        match input {
            TerminalEvent::FocusLost => {
                self.focus = TerminalFocus::Lost;
                self.keymap.cancel_pending();
            }
            TerminalEvent::FocusGained => self.focus = TerminalFocus::Gained,
            TerminalEvent::Key(key) => {
                if let Some((owner, command)) = self.resolve_key((*key).into()) {
                    effects = effects.send(owner, command);
                }
            }
            _ => {}
        }
        self.apply_effects(effects)
    }

    pub fn apply_event(&mut self, event: &U::Event) -> DispatchResult<U::Operation> {
        let mut pending = Vec::new();
        self.notify(std::slice::from_ref(event), &mut pending);
        self.dispatch(pending)
    }

    pub(crate) fn apply_effects(&mut self, effects: Effects<U>) -> DispatchResult<U::Operation> {
        let mut pending = Vec::new();
        self.enqueue(effects, &mut pending);
        self.dispatch(pending)
    }

    fn enqueue(&mut self, mut effects: Effects<U>, pending: &mut Vec<Dispatch<U>>) {
        let events = std::mem::take(&mut effects.events);
        // The originating reply runs after the replies to the facts it emitted.
        Dispatch::push_reply(pending, effects);
        self.notify(&events, pending);
    }

    fn dispatch(&mut self, mut pending: Vec<Dispatch<U>>) -> DispatchResult<U::Operation> {
        let mut operations = Vec::new();
        let mut control_flow = ControlFlow::Continue(());
        while let Some(next) = pending.pop() {
            match next {
                Dispatch::Reply(reply) => {
                    if reply.quit {
                        control_flow = ControlFlow::Break(());
                    }
                    operations.extend(reply.operations);
                    pending.extend(
                        reply
                            .commands
                            .into_iter()
                            .rev()
                            .map(|(id, cmd)| Dispatch::Command(id, cmd)),
                    );
                }
                Dispatch::Command(to, command) => {
                    // A preceding command can have closed the addressed component.
                    if self.ui.component(to).is_some() {
                        let reply = self.ui.apply_command(to, command);
                        self.enqueue(reply, &mut pending);
                    }
                }
            }
        }
        self.refresh();
        DispatchResult {
            operations,
            control_flow,
        }
    }

    /// Finish notifying every subscriber before any of their commands runs.
    fn notify(&mut self, events: &[U::Event], pending: &mut Vec<Dispatch<U>>) {
        if events.is_empty() {
            return;
        }
        let first_reply = pending.len();
        let mut notifications = vec![Notification::new(Publication::Borrowed(events))];
        while let Some(notification) = notifications.last_mut() {
            let Some((id, event)) = notification.next(&self.ui) else {
                let finished = notifications.pop().expect("current notification");
                if let Publication::Reply(mut reply) = finished.publication {
                    reply.events.clear();
                    Dispatch::push_reply(pending, reply);
                }
                continue;
            };
            if let Some(component) = self.ui.component_mut(id)
                && component.subscribes(event)
            {
                let reply = component.apply_event(event);
                if reply.events.is_empty() {
                    Dispatch::push_reply(pending, reply);
                } else {
                    notifications.push(Notification::new(Publication::Reply(reply)));
                }
            }
        }
        // Replies were collected in delivery order. The dispatch stack pops from its end.
        pending[first_reply..].reverse();
    }
}

enum Dispatch<U: Ui> {
    Reply(Effects<U>),
    Command(U::Id, U::Command),
}

impl<U: Ui> Dispatch<U> {
    fn push_reply(pending: &mut Vec<Self>, reply: Effects<U>) {
        if reply.quit || !reply.commands.is_empty() || !reply.operations.is_empty() {
            pending.push(Self::Reply(reply));
        }
    }
}

/// External events can be borrowed; a component's reply owns its nested events.
enum Publication<'a, U: Ui> {
    Borrowed(&'a [U::Event]),
    Reply(Effects<U>),
}

/// The sibling subscribers and events to resume after a nested notification.
struct Notification<'a, U: Ui> {
    publication: Publication<'a, U>,
    event: usize,
    recipients: Option<std::vec::IntoIter<U::Id>>,
}

impl<'a, U: Ui> Notification<'a, U> {
    fn new(publication: Publication<'a, U>) -> Self {
        Self {
            publication,
            event: 0,
            recipients: None,
        }
    }

    fn next(&mut self, ui: &U) -> Option<(U::Id, &U::Event)> {
        let events = match &self.publication {
            Publication::Borrowed(events) => *events,
            Publication::Reply(reply) => &reply.events,
        };
        while let Some(event) = events.get(self.event) {
            let ids = self
                .recipients
                .get_or_insert_with(|| ui.ids().into_iter().collect::<Vec<_>>().into_iter());
            if let Some(id) = ids.next() {
                return Some((id, event));
            }
            self.event += 1;
            self.recipients = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, UiKeymap};

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Id {
        Window,
        Tab,
    }

    enum Event {
        Count(u16),
        Open,
        Close,
        Opened,
        SelectTab,
    }
    #[derive(Clone)]
    enum Command {
        Count(u16),
        Select,
    }

    #[derive(Default)]
    struct Window {
        tab: Option<Tab>,
    }
    #[derive(Default)]
    struct Tab {
        saw_selection: bool,
    }

    impl Ui for Window {
        type Id = Id;
        type Command = Command;
        type Event = Event;
        type Operation = &'static str;
        type Theme = ();
        type Layer = ();
        type Keymap = UiKeymap<Self>;
        type Context<'a> = RenderContext<'a, Self>;

        fn theme(&self) -> &() {
            &()
        }
        fn ids(&self) -> impl IntoIterator<Item = Id> {
            std::iter::once(Id::Window).chain(self.tab.as_ref().map(|_| Id::Tab))
        }
        fn visible(&self) -> impl IntoIterator<Item = Id> {
            [Id::Window]
        }
        fn component(&self, id: Id) -> Option<&dyn Component<Self>> {
            match id {
                Id::Window => Some(self),
                Id::Tab => self.tab.as_ref().map(|tab| tab as &dyn Component<Self>),
            }
        }
        fn component_mut(&mut self, id: Id) -> Option<&mut dyn Component<Self>> {
            match id {
                Id::Window => Some(self),
                Id::Tab => self.tab.as_mut().map(|tab| tab as &mut dyn Component<Self>),
            }
        }
        fn apply_command(&mut self, to: Id, command: Command) -> Effects<Self> {
            self.component_mut(to)
                .expect("a command has a live recipient")
                .apply_command(command)
        }
    }

    impl Component<Window> for Window {
        fn render(&self, _: &mut Frame<'_>, _: &RenderContext<'_, Window>) {}
        fn subscribes(&self, _: &Event) -> bool {
            true
        }
        fn apply_event(&mut self, event: &Event) -> Effects<Window> {
            match event {
                Event::Count(0) => Effects::NONE.send(Id::Window, Command::Count(20_000)),
                Event::Count(n) => Effects::NONE.emit(Event::Count(n - 1)),
                Event::Open => {
                    self.tab = Some(Tab::default());
                    Effects::NONE.emit(Event::Opened)
                }
                Event::Close => {
                    self.tab = None;
                    Effects::NONE.send(Id::Tab, Command::Select)
                }
                Event::Opened => Effects::NONE,
                Event::SelectTab => Effects::NONE
                    .emit(Event::Opened)
                    .operation("window")
                    .send(Id::Tab, Command::Select),
            }
        }
        fn apply_command(&mut self, command: Command) -> Effects<Window> {
            match command {
                Command::Count(0) => Effects::NONE.operation("finished"),
                Command::Count(n) => Effects::NONE.send(Id::Window, Command::Count(n - 1)),
                Command::Select => unreachable!(),
            }
        }
    }

    impl Component<Window> for Tab {
        fn render(&self, _: &mut Frame<'_>, _: &RenderContext<'_, Window>) {}
        fn subscribes(&self, _: &Event) -> bool {
            true
        }
        fn apply_event(&mut self, event: &Event) -> Effects<Window> {
            match event {
                Event::Opened => Effects::NONE.operation("opened"),
                Event::SelectTab => {
                    self.saw_selection = true;
                    Effects::NONE.operation("tab")
                }
                _ => panic!("tab received an event outside its lifetime"),
            }
        }
        fn apply_command(&mut self, command: Command) -> Effects<Window> {
            assert!(matches!(command, Command::Select));
            assert!(
                self.saw_selection,
                "publish to every subscriber before running commands"
            );
            Effects::NONE.operation("selected")
        }
    }

    #[test]
    fn finite_event_and_command_chains_finish_without_using_the_call_stack() {
        let mut ui = Compositor::new(Window::default(), UiKeymap::<Window>::default());
        assert_eq!(
            ui.apply_event(&Event::Count(20_000)).operations,
            ["finished"]
        );
    }

    #[test]
    fn nested_events_see_new_tabs_and_closing_a_tab_drops_late_deliveries() {
        let mut ui = Compositor::new(Window::default(), UiKeymap::<Window>::default());
        assert_eq!(ui.apply_event(&Event::Open).operations, ["opened"]);
        assert_eq!(ui.apply_event(&Event::Close).operations, [] as [&str; 0]);
    }

    #[test]
    fn nested_replies_run_before_their_parent_and_its_siblings() {
        let window = Window {
            tab: Some(Tab::default()),
        };
        let mut ui = Compositor::new(window, UiKeymap::<Window>::default());
        assert_eq!(
            ui.apply_event(&Event::SelectTab).operations,
            ["opened", "window", "selected", "tab"]
        );
    }
}
