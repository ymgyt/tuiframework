use crate::Components;

/// Ordered layers drawn from back to front. Name an overlay to receive [`crate::Effects::open`].
/// Overlay IDs and stack IDs must each be unique across the entire layout.
#[must_use]
pub struct Overlay<C: Components> {
    name: Option<C::OverlayId>,
    layers: Vec<Layer<C>>,
}

type BranchSelector<C> = Box<dyn Fn(&C) -> Option<usize>>;

enum Layer<C: Components> {
    Component(C::Id),
    Stack {
        name: C::StackId,
        base: Overlay<C>,
        history: Vec<C::Id>,
    },
    Switch {
        select: BranchSelector<C>,
        branches: Vec<Overlay<C>>,
    },
}

pub(crate) enum DisplayChange<C: Components> {
    Open(C::OverlayId),
    Push(C::StackId),
    Close,
}

impl<C: Components> Default for Overlay<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Components> Overlay<C> {
    pub fn new() -> Self {
        Self {
            name: None,
            layers: Vec::new(),
        }
    }

    pub fn named(id: C::OverlayId) -> Self {
        Self {
            name: Some(id),
            ..Self::new()
        }
    }

    pub fn component(mut self, id: C::Id) -> Self {
        self.layers.push(Layer::Component(id));
        self
    }

    /// Shows the base until a component is pushed; closing it reveals its predecessor.
    pub fn stack(mut self, id: C::StackId, base: Self) -> Self {
        self.layers.push(Layer::Stack {
            name: id,
            base,
            history: Vec::new(),
        });
        self
    }

    /// Draws the branch selected by the application. An unmatched value draws no branch.
    pub fn switch<K: Eq + 'static>(
        mut self,
        selected: impl Fn(&C) -> K + 'static,
        branches: impl IntoIterator<Item = (K, Self)>,
    ) -> Self {
        let (keys, branches): (Vec<_>, Vec<_>) = branches.into_iter().unzip();
        for (i, key) in keys.iter().enumerate() {
            assert!(!keys[..i].contains(key), "duplicate switch branch");
        }
        self.layers.push(Layer::Switch {
            select: Box::new(move |components| {
                let current = selected(components);
                keys.iter().position(|key| *key == current)
            }),
            branches,
        });
        self
    }

    pub(crate) fn check_names(
        &self,
        overlays: &mut Vec<C::OverlayId>,
        stacks: &mut Vec<C::StackId>,
    ) {
        if let Some(id) = self.name {
            assert!(!overlays.contains(&id), "duplicate overlay ID");
            overlays.push(id);
        }
        for layer in &self.layers {
            match layer {
                Layer::Component(_) => {}
                Layer::Stack { name, base, .. } => {
                    assert!(!stacks.contains(name), "duplicate stack ID");
                    stacks.push(*name);
                    base.check_names(overlays, stacks);
                }
                Layer::Switch { branches, .. } => {
                    for branch in branches {
                        branch.check_names(overlays, stacks);
                    }
                }
            }
        }
    }

    pub(crate) fn shown(&self, components: &C, output: &mut Vec<C::Id>) {
        for layer in &self.layers {
            match layer {
                Layer::Component(id) => output.push(*id),
                Layer::Stack { base, history, .. } => match history
                    .iter()
                    .rev()
                    .find(|id| components.get(**id).is_some())
                {
                    Some(id) => output.push(*id),
                    None => base.shown(components, output),
                },
                Layer::Switch { select, branches } => {
                    if let Some(index) = select(components) {
                        branches[index].shown(components, output);
                    }
                }
            }
        }
    }

    pub(crate) fn apply(&mut self, sender: C::Id, change: &DisplayChange<C>) {
        self.remove(sender);
        match change {
            DisplayChange::Open(name) => {
                let target = self
                    .overlay_mut(*name)
                    .expect("open requires a declared overlay ID");
                target.layers.push(Layer::Component(sender));
            }
            DisplayChange::Push(name) => {
                let history = self
                    .stack_mut(*name)
                    .expect("push requires a declared stack ID");
                history.push(sender);
            }
            DisplayChange::Close => {}
        }
    }

    fn remove(&mut self, target: C::Id) {
        self.layers
            .retain(|layer| !matches!(layer, Layer::Component(id) if *id == target));
        for layer in &mut self.layers {
            match layer {
                Layer::Stack { history, base, .. } => {
                    history.retain(|id| *id != target);
                    base.remove(target);
                }
                Layer::Switch { branches, .. } => {
                    for branch in branches {
                        branch.remove(target);
                    }
                }
                Layer::Component(_) => {}
            }
        }
    }

    fn overlay_mut(&mut self, target: C::OverlayId) -> Option<&mut Self> {
        if self.name == Some(target) {
            return Some(self);
        }
        self.layers
            .iter_mut()
            .flat_map(Layer::children_mut)
            .find_map(|child| child.overlay_mut(target))
    }

    fn stack_mut(&mut self, target: C::StackId) -> Option<&mut Vec<C::Id>> {
        self.layers.iter_mut().find_map(|layer| match layer {
            Layer::Component(_) => None,
            Layer::Stack {
                name,
                history,
                base,
            } => {
                if *name == target {
                    Some(history)
                } else {
                    base.stack_mut(target)
                }
            }
            Layer::Switch { branches, .. } => branches
                .iter_mut()
                .find_map(|child| child.stack_mut(target)),
        })
    }
}

impl<C: Components> Layer<C> {
    fn children_mut(&mut self) -> &mut [Overlay<C>] {
        match self {
            Self::Component(_) => &mut [],
            Self::Stack { base, .. } => std::slice::from_mut(base),
            Self::Switch { branches, .. } => branches,
        }
    }
}
