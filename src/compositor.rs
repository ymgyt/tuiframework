use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::Frame;

use crate::{
    component::{Components, Effects, KeyAction as _, RenderContext},
    keymap::{KeyBindings as _, KeyStroke, LayerStack},
};

/// Routes inputs and facts to the app's components and draws the visible stack.
pub struct Compositor<A: Components> {
    pub components: A,
    keymap: A::Keymap,
    visible: Vec<A::Id>,
    layers: LayerStack<A::Layer, A::Id>,
    focused: bool,
    pub(crate) quit: bool,
}

impl<A: Components> Compositor<A> {
    pub fn new(components: A, keymap: A::Keymap) -> Self {
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
            if let Some(component) = self.components.component(id) {
                component.mask_lower_layers(&mut self.layers);
                self.layers.extend(id, component.key_layers());
            }
        }
        self.keymap.sync_layers(&self.layers);
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let cx = RenderContext {
            components: &self.components,
            theme: self.components.theme(),
            area: frame.area(),
            focused: self.focused,
            keymap: &self.keymap,
            layers: &self.layers,
        };
        let view = A::View::from(cx);
        for &id in &self.visible {
            if let Some(component) = self.components.component(id) {
                component.render(frame, &view);
            }
        }
    }

    pub fn resolve_key(&mut self, key: KeyStroke) -> Option<(A::Id, A::Command)> {
        let (layer, action) = self.keymap.binding(&self.layers, key)?;
        Some(action.command_for(self.layers.owner(layer)?))
    }

    pub(crate) fn send(&mut self, to: A::Id, command: A::Command) -> Vec<A::Operation> {
        self.dispatch(vec![Dispatch::Command(to, command)])
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
        let mut pending = Vec::new();
        self.notify(std::slice::from_ref(event), &mut pending);
        self.dispatch(pending)
    }

    fn apply(&mut self, effects: Effects<A>) -> Vec<A::Operation> {
        let mut pending = Vec::new();
        self.enqueue(effects, &mut pending);
        self.dispatch(pending)
    }

    fn enqueue(&mut self, mut effects: Effects<A>, pending: &mut Vec<Dispatch<A>>) {
        let events = std::mem::take(&mut effects.events);
        // The originating reply runs after the replies to the facts it emitted.
        Dispatch::push_reply(pending, effects);
        self.notify(&events, pending);
    }

    fn dispatch(&mut self, mut pending: Vec<Dispatch<A>>) -> Vec<A::Operation> {
        let mut operations = Vec::new();
        while let Some(next) = pending.pop() {
            match next {
                Dispatch::Reply(reply) => {
                    self.quit |= reply.quit;
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
                    if self.components.component(to).is_some() {
                        let reply = self.components.command(to, command);
                        self.enqueue(reply, &mut pending);
                    }
                }
            }
        }
        self.refresh();
        operations
    }

    /// Finish notifying every subscriber before any of their commands runs.
    fn notify(&mut self, events: &[A::Event], pending: &mut Vec<Dispatch<A>>) {
        if events.is_empty() {
            return;
        }
        let first_reply = pending.len();
        let mut notifications = vec![Notification::new(Publication::Borrowed(events))];
        while let Some(notification) = notifications.last_mut() {
            let Some((id, event)) = notification.next(&self.components) else {
                let finished = notifications.pop().expect("current notification");
                if let Publication::Reply(mut reply) = finished.publication {
                    reply.events.clear();
                    Dispatch::push_reply(pending, reply);
                }
                continue;
            };
            if let Some(component) = self.components.component_mut(id)
                && component.subscribes(event)
            {
                let reply = component.on_event(event);
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

enum Dispatch<A: Components> {
    Reply(Effects<A>),
    Command(A::Id, A::Command),
}

impl<A: Components> Dispatch<A> {
    fn push_reply(pending: &mut Vec<Self>, reply: Effects<A>) {
        if reply.quit || !reply.commands.is_empty() || !reply.operations.is_empty() {
            pending.push(Self::Reply(reply));
        }
    }
}

/// External events can be borrowed; a component's reply owns its nested events.
enum Publication<'a, A: Components> {
    Borrowed(&'a [A::Event]),
    Reply(Effects<A>),
}

/// The sibling subscribers and events to resume after a nested notification.
struct Notification<'a, A: Components> {
    publication: Publication<'a, A>,
    event: usize,
    recipients: Option<std::vec::IntoIter<A::Id>>,
}

impl<'a, A: Components> Notification<'a, A> {
    fn new(publication: Publication<'a, A>) -> Self {
        Self {
            publication,
            event: 0,
            recipients: None,
        }
    }

    fn next(&mut self, components: &A) -> Option<(A::Id, &A::Event)> {
        let events = match &self.publication {
            Publication::Borrowed(events) => *events,
            Publication::Reply(reply) => &reply.events,
        };
        while let Some(event) = events.get(self.event) {
            let ids = self.recipients.get_or_insert_with(|| {
                components.ids().into_iter().collect::<Vec<_>>().into_iter()
            });
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
    use crate::{AppKeymap, Component};

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

    impl Components for Window {
        type Id = Id;
        type Command = Command;
        type Event = Event;
        type Operation = &'static str;
        type Theme = ();
        type Layer = ();
        type Keymap = AppKeymap<Self>;
        type View<'a> = RenderContext<'a, Self>;

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
        fn command(&mut self, to: Id, command: Command) -> Effects<Self> {
            self.component_mut(to)
                .expect("a command has a live recipient")
                .command(command)
        }
    }

    impl Component<Window> for Window {
        fn render(&self, _: &mut Frame<'_>, _: &RenderContext<'_, Window>) {}
        fn subscribes(&self, _: &Event) -> bool {
            true
        }
        fn on_event(&mut self, event: &Event) -> Effects<Window> {
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
        fn command(&mut self, command: Command) -> Effects<Window> {
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
        fn on_event(&mut self, event: &Event) -> Effects<Window> {
            match event {
                Event::Opened => Effects::NONE.operation("opened"),
                Event::SelectTab => {
                    self.saw_selection = true;
                    Effects::NONE.operation("tab")
                }
                _ => panic!("tab received an event outside its lifetime"),
            }
        }
        fn command(&mut self, command: Command) -> Effects<Window> {
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
        let mut ui = Compositor::new(Window::default(), AppKeymap::<Window>::default());
        assert_eq!(ui.event(&Event::Count(20_000)), ["finished"]);
    }

    #[test]
    fn nested_events_see_new_tabs_and_closing_a_tab_drops_late_deliveries() {
        let mut ui = Compositor::new(Window::default(), AppKeymap::<Window>::default());
        assert_eq!(ui.event(&Event::Open), ["opened"]);
        assert_eq!(ui.event(&Event::Close), [] as [&str; 0]);
    }

    #[test]
    fn nested_replies_run_before_their_parent_and_its_siblings() {
        let window = Window {
            tab: Some(Tab::default()),
        };
        let mut ui = Compositor::new(window, AppKeymap::<Window>::default());
        assert_eq!(
            ui.event(&Event::SelectTab),
            ["opened", "window", "selected", "tab"]
        );
    }
}
