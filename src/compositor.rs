use crate::{
    ApplicationEffects, Components, Effects, Overlay, RenderInputs,
    keymap::{KeyBindings as _, KeyStroke, LayerStack},
};
use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::Frame;
use std::ops::ControlFlow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFocus {
    Gained,
    Lost,
}

/// Operations and loop control resulting from a screen transition.
#[derive(Debug)]
#[must_use]
pub struct DispatchResult<O> {
    pub operations: Vec<O>,
    pub control_flow: ControlFlow<()>,
}

impl<O> Default for DispatchResult<O> {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            control_flow: ControlFlow::Continue(()),
        }
    }
}

/// Owns screen composition and input routing. Also available as a headless test fixture.
pub struct Compositor<C: Components> {
    pub components: C,
    keymap: C::Keymap,
    overlay: Overlay<C>,
    shown: Vec<C::Id>,
    layers: LayerStack<C::Layer, C::Id>,
    focus: TerminalFocus,
}

impl<C: Components> Compositor<C> {
    pub fn new(components: C, overlay: Overlay<C>, keymap: C::Keymap) -> Self {
        overlay.check_names(&mut Vec::new(), &mut Vec::new());
        let mut screens = Self {
            components,
            overlay,
            keymap,
            shown: Vec::new(),
            layers: LayerStack::default(),
            focus: TerminalFocus::Gained,
        };
        screens.refresh();
        screens
    }

    pub fn refresh(&mut self) {
        self.shown.clear();
        self.overlay.shown(&self.components, &mut self.shown);
        self.shown.retain(|id| self.components.get(*id).is_some());
        let registered: Vec<_> = self.components.ids().into_iter().collect();
        for id in registered {
            if let Some(component) = self.components.get_mut(id) {
                component.update_keymap(&mut self.keymap);
            }
        }
        self.layers.clear();
        for &id in &self.shown {
            if let Some(component) = self.components.get(id) {
                component.mask_lower_layers(&mut self.layers);
                self.layers.extend(id, component.key_layers());
            }
        }
        self.keymap.sync_layers(&self.layers);
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let cx = C::RenderContext::from(RenderInputs {
            components: &self.components,
            area: frame.area(),
            focus: self.focus,
            keymap: &self.keymap,
            layers: &self.layers,
        });
        for &id in &self.shown {
            if let Some(component) = self.components.get(id) {
                component.render(frame, &cx);
            }
        }
    }

    pub fn resolve_key(&mut self, key: KeyStroke) -> Option<(C::Id, C::Command)> {
        self.keymap.resolve_command(&self.layers, key)
    }

    pub fn is_busy(&self) -> bool {
        self.components.ids().into_iter().any(|id| {
            self.components
                .get(id)
                .is_some_and(crate::Component::is_busy)
        })
    }

    pub(crate) fn tick(&mut self) {
        let ids: Vec<_> = self.components.ids().into_iter().collect();
        for id in ids {
            if let Some(component) = self.components.get_mut(id)
                && component.is_busy()
            {
                component.tick();
            }
        }
        self.refresh();
    }

    pub fn apply_terminal_event(&mut self, input: &TerminalEvent) -> DispatchResult<C::Operation> {
        let mut result = DispatchResult::default();
        if matches!(input, TerminalEvent::Key(key) if key.kind == KeyEventKind::Release) {
            return result;
        }
        let command = match input {
            TerminalEvent::FocusLost => {
                self.focus = TerminalFocus::Lost;
                self.keymap.cancel_pending();
                None
            }
            TerminalEvent::FocusGained => {
                self.focus = TerminalFocus::Gained;
                None
            }
            TerminalEvent::Key(key) => self.resolve_key((*key).into()),
            _ => None,
        };
        let top = self.shown.last().copied();
        let ids: Vec<_> = self.components.ids().into_iter().collect();
        for id in ids {
            if let Some(component) = self.components.get_mut(id) {
                let mut effects = Effects::default();
                component.on_input(input, Some(id) == top, &mut effects);
                self.finish(id, effects, &mut result);
            }
        }
        if let Some((id, command)) = command {
            self.command(id, command, &mut result);
        }
        self.refresh();
        result
    }

    fn command(
        &mut self,
        id: C::Id,
        command: C::Command,
        result: &mut DispatchResult<C::Operation>,
    ) {
        if let Some(component) = self.components.get_mut(id) {
            let mut effects = Effects::default();
            component.apply_command(command, &mut effects);
            self.finish(id, effects, result);
        }
    }

    pub fn apply_command(
        &mut self,
        id: C::Id,
        command: C::Command,
    ) -> DispatchResult<C::Operation> {
        let mut result = DispatchResult::default();
        self.command(id, command, &mut result);
        self.refresh();
        result
    }

    pub fn apply_event(&mut self, event: &C::Event) -> DispatchResult<C::Operation> {
        let mut result = DispatchResult::default();
        self.notify(std::slice::from_ref(event), &mut result);
        self.refresh();
        result
    }

    pub(crate) fn apply_effects(
        &mut self,
        effects: ApplicationEffects<C>,
    ) -> DispatchResult<C::Operation> {
        let mut result = DispatchResult::default();
        self.notify(&effects.events, &mut result);
        result.extend(effects.operations, effects.quit);
        self.refresh();
        result
    }

    fn finish(
        &mut self,
        id: C::Id,
        effects: Effects<C>,
        result: &mut DispatchResult<C::Operation>,
    ) {
        for change in effects.display {
            self.overlay.apply(id, &change);
        }
        if !effects.app.events.is_empty() {
            self.notify(&effects.app.events, result);
        }
        result.extend(effects.app.operations, effects.app.quit);
    }

    /// Nested events complete before the next subscriber sees the parent event.
    /// A new event snapshots the current registry; removed recipients are skipped.
    fn notify(&mut self, events: &[C::Event], result: &mut DispatchResult<C::Operation>) {
        let mut pending = vec![Notification::new(Publication::Borrowed(events))];
        while let Some(current) = pending.last_mut() {
            let Some((id, event)) = current.next(&self.components) else {
                if let Some(Notification {
                    publication: Publication::Reply(reply),
                    ..
                }) = pending.pop()
                {
                    result.extend(reply.operations, reply.quit);
                }
                continue;
            };
            if let Some(component) = self.components.get_mut(id)
                && component.subscribes(event)
            {
                let mut effects = Effects::default();
                component.apply_event(event, &mut effects);
                for change in effects.display {
                    self.overlay.apply(id, &change);
                }
                if effects.app.events.is_empty() {
                    result.extend(effects.app.operations, effects.app.quit);
                } else {
                    pending.push(Notification::new(Publication::Reply(effects.app)));
                }
            }
        }
    }
}

impl<O> DispatchResult<O> {
    fn extend(&mut self, operations: Vec<O>, quit: bool) {
        self.operations.extend(operations);
        if quit {
            self.control_flow = ControlFlow::Break(());
        }
    }
}

/// External events can be borrowed; a component's reply owns its nested events.
enum Publication<'a, U: Components> {
    Borrowed(&'a [U::Event]),
    Reply(ApplicationEffects<U>),
}

/// The sibling subscribers and events to resume after a nested notification.
struct Notification<'a, U: Components> {
    publication: Publication<'a, U>,
    event: usize,
    recipients: Option<std::vec::IntoIter<U::Id>>,
}

impl<'a, U: Components> Notification<'a, U> {
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

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
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
        Push,
        Open,
        Close,
    }

    #[derive(Default)]
    struct Window {
        tab: Option<Tab>,
    }
    #[derive(Default)]
    struct Tab;

    impl Components for Window {
        type Id = Id;
        type Command = Command;
        type Event = Event;
        type Operation = &'static str;
        type OverlayId = ();
        type StackId = ();
        type Layer = ();
        type Keymap = UiKeymap<Self>;
        type RenderContext<'a> = RenderInputs<'a, Self>;

        fn ids(&self) -> impl IntoIterator<Item = Id> {
            std::iter::once(Id::Window).chain(self.tab.as_ref().map(|_| Id::Tab))
        }
        fn get(&self, id: Id) -> Option<&dyn Component<Self>> {
            match id {
                Id::Window => Some(self),
                Id::Tab => self.tab.as_ref().map(|tab| tab as &dyn Component<Self>),
            }
        }
        fn get_mut(&mut self, id: Id) -> Option<&mut dyn Component<Self>> {
            match id {
                Id::Window => Some(self),
                Id::Tab => self.tab.as_mut().map(|tab| tab as &mut dyn Component<Self>),
            }
        }
    }

    impl Component<Window> for Window {
        fn render(&self, _: &mut Frame<'_>, _: &RenderInputs<'_, Window>) {}
        fn subscribes(&self, _: &Event) -> bool {
            true
        }
        fn apply_event(&mut self, event: &Event, effects: &mut Effects<Window>) {
            match event {
                Event::Count(0) => effects.operation("finished"),
                Event::Count(n) => effects.emit(Event::Count(n - 1)),
                Event::Open => {
                    self.tab = Some(Tab);
                    effects.emit(Event::Opened);
                }
                Event::Close => {
                    self.tab = None;
                }
                Event::Opened => {}
                Event::SelectTab => {
                    effects.emit(Event::Opened);
                    effects.operation("window");
                }
            }
        }
    }

    impl Component<Window> for Tab {
        fn render(&self, _: &mut Frame<'_>, _: &RenderInputs<'_, Window>) {}
        fn subscribes(&self, _: &Event) -> bool {
            true
        }
        fn apply_event(&mut self, event: &Event, effects: &mut Effects<Window>) {
            match event {
                Event::Opened => effects.operation("opened"),
                Event::SelectTab => {
                    effects.operation("tab");
                }
                _ => panic!("tab received an event outside its lifetime"),
            }
        }
        fn key_layers(&self) -> &[()] {
            &[()]
        }
        fn apply_command(&mut self, command: Command, effects: &mut Effects<Window>) {
            match command {
                Command::Push => effects.push(()),
                Command::Open => effects.open(()),
                Command::Close => effects.close(),
            }
        }
    }

    #[test]
    fn finite_event_chains_finish_without_using_the_call_stack() {
        let mut ui = Compositor::new(
            Window::default(),
            Overlay::new().component(Id::Window),
            UiKeymap::<Window>::default(),
        );
        assert_eq!(
            ui.apply_event(&Event::Count(20_000)).operations,
            ["finished"]
        );
    }

    #[test]
    fn nested_events_see_new_tabs_and_closing_a_tab_drops_late_deliveries() {
        let mut ui = Compositor::new(
            Window::default(),
            Overlay::new().component(Id::Window),
            UiKeymap::<Window>::default(),
        );
        assert_eq!(ui.apply_event(&Event::Open).operations, ["opened"]);
        assert_eq!(ui.apply_event(&Event::Close).operations, [] as [&str; 0]);
    }

    #[test]
    fn nested_replies_run_before_their_parent_and_its_siblings() {
        let window = Window { tab: Some(Tab) };
        let mut ui = Compositor::new(
            window,
            Overlay::new().component(Id::Window),
            UiKeymap::<Window>::default(),
        );
        assert_eq!(
            ui.apply_event(&Event::SelectTab).operations,
            ["opened", "window", "tab"]
        );
    }

    #[test]
    fn a_nested_stack_and_popup_share_the_active_branch_with_input() {
        let window = Window { tab: Some(Tab) };
        let branch = Overlay::named(()).stack((), Overlay::new().component(Id::Window));
        let overlay = Overlay::new().switch(
            |window: &Window| window.tab.is_some(),
            [
                (true, branch),
                (false, Overlay::new().component(Id::Window)),
            ],
        );
        let mut screens = Compositor::new(window, overlay, UiKeymap::<Window>::default());
        let _ = screens.apply_command(Id::Tab, Command::Push);
        assert_eq!(screens.shown, [Id::Tab]);
        assert_eq!(screens.layers.owner(()), Some(Id::Tab));
        let _ = screens.apply_command(Id::Tab, Command::Open);
        assert_eq!(screens.shown, [Id::Window, Id::Tab]);
        screens.components.tab = None;
        screens.refresh();
        assert_eq!(screens.shown, [Id::Window]);
        assert_eq!(screens.layers.owner(()), None);
        screens.components.tab = Some(Tab);
        screens.refresh();
        let _ = screens.apply_command(Id::Tab, Command::Close);
        assert_eq!(screens.shown, [Id::Window]);
    }
}
