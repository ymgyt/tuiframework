/// The key layers contributed by the visible components, back to front.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerStack<L, I> {
    pub(super) entries: Vec<LayerOwner<L, I>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LayerOwner<L, I> {
    pub(super) layer: L,
    pub(super) owner: I,
    pub(super) covered: bool,
}

impl<L, I> Default for LayerStack<L, I> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<L: Copy + Eq, I: Copy> LayerStack<L, I> {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// The frontmost component to declare a layer owns that layer's commands.
    pub fn extend(&mut self, owner: I, layers: &[L]) {
        for &layer in layers {
            self.entries.retain(|entry| entry.layer != layer);
            self.entries.push(LayerOwner {
                layer,
                owner,
                covered: false,
            });
        }
    }

    /// Keeps these layers' keys consumed while their components are covered.
    pub fn cover(&mut self, layers: &[L]) {
        for entry in &mut self.entries {
            if layers.contains(&entry.layer) {
                entry.covered = true;
            }
        }
    }

    /// Removes everything above the named layer, leaving its bindings available.
    pub fn truncate_after(&mut self, layer: L) {
        if let Some(index) = self.entries.iter().position(|entry| entry.layer == layer) {
            self.entries.truncate(index + 1);
        }
    }

    /// Layers from back to front, including reserved layers covered by a popup.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = L> + '_ {
        self.entries.iter().map(|entry| entry.layer)
    }

    /// Returns the component that can receive a command from this layer.
    /// Covered and inactive layers have no available owner.
    pub fn owner(&self, layer: L) -> Option<I> {
        self.entries
            .iter()
            .find(|entry| entry.layer == layer && !entry.covered)
            .map(|entry| entry.owner)
    }

    pub fn top(&self) -> Option<L> {
        self.entries.last().map(|entry| entry.layer)
    }
}
