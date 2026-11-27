//! Layered key sequences, user overrides, and hints derived from reachable bindings.

mod config;
mod key;
mod layers;
mod trie;

use std::{collections::HashMap, hash::Hash};

use thiserror::Error;

use crate::Components;

pub use config::{Binding, KeymapConfig};
pub use key::{KeySequence, KeyStroke};
use layers::LayerOwner;
pub use layers::LayerStack;
pub use trie::LayerKeymap;
use trie::Lookup;

/// Keymap built from the command, layer, and component types of a UI.
pub type UiKeymap<U, B = <U as Components>::Command> =
    Keymap<<U as Components>::Layer, B, <U as Components>::Id>;

/// Resolves a binding to its destination and command once the active layer's
/// owner is known. Binding a command directly sends it to that owner.
pub trait KeyAction<U: Components> {
    fn command_for(self, owner: U::Id) -> (U::Id, U::Command);
}

impl<U: Components> KeyAction<U> for U::Command {
    fn command_for(self, owner: U::Id) -> (U::Id, U::Command) {
        (owner, self)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeymapError {
    #[error("invalid key notation '{0}': {1}")]
    InvalidKeyNotation(String, String),
    #[error("a key sequence must contain at least one key")]
    EmptyKeySequence,
    #[error("duplicate binding '{0}'")]
    Duplicate(KeySequence),
    #[error("binding '{0}' conflicts with a prefix in the same layer")]
    Ambiguous(KeySequence),
}

/// An application's compiled bindings. The compositor supplies the active layers
/// and delivers a match to the component that owns its layer.
pub trait KeyBindings<U: Components> {
    /// Resolves one key to its destination and domain command.
    fn resolve_command(
        &mut self,
        layers: &LayerStack<U::Layer, U::Id>,
        key: KeyStroke,
    ) -> Option<(U::Id, U::Command)>;

    /// Discards an unfinished sequence, for example when terminal focus is lost.
    fn cancel_pending(&mut self) {}

    /// Invalidates any sequence that no longer belongs to the active layers.
    fn sync_layers(&mut self, _layers: &LayerStack<U::Layer, U::Id>) {}
}

/// The result of a key press in the current set of layers.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolution<L, I, C> {
    /// The winning layer, its frontmost component, and the bound command.
    Matched {
        layer: L,
        owner: I,
        command: C,
    },
    Pending,
    Cancelled,
    Masked,
    Unbound,
}

/// A reachable binding, or its remaining suffix during a multi-key sequence.
pub struct KeyHint<'a, L, C> {
    pub layer: L,
    pub keys: String,
    pub command: &'a C,
    pub description: Option<&'a str>,
}

/// Resolves input against the active layers and tracks the owner of a pending prefix.
#[derive(Clone, Debug)]
pub struct Keymap<L, C, I> {
    layers: HashMap<L, LayerKeymap<C>>,
    overrides: HashMap<L, LayerKeymap<C>>,
    pending: Option<Pending<L, I>>,
}

#[derive(Clone, Debug)]
struct Pending<L, I> {
    layer: L,
    stack: LayerStack<L, I>,
    keys: Vec<KeyStroke>,
}

impl<L, C, I> Default for Keymap<L, C, I> {
    fn default() -> Self {
        Self {
            layers: HashMap::new(),
            overrides: HashMap::new(),
            pending: None,
        }
    }
}

impl<L: Copy + Eq + Hash, C: Clone, I: Copy + Eq> Keymap<L, C, I> {
    pub fn compile(config: KeymapConfig<L, C>) -> Result<Self, KeymapError> {
        let mut layers = HashMap::new();
        for (layer, config) in config.layers {
            let mut map = LayerKeymap::default();
            for binding in config.keymap {
                map.bind(binding)?;
            }
            layers.insert(layer, map);
        }
        Ok(Self {
            layers,
            ..Self::default()
        })
    }

    /// Completes local prefixes with compatible bindings from a shared layer.
    /// For example, overriding `g g` locally can preserve the shared `g e`.
    pub fn inherit_shared_prefixes(&mut self, layer: L, shared: L) {
        if layer != shared
            && let [Some(local), Some(base)] = self.layers.get_disjoint_mut([&layer, &shared])
        {
            local.inherit_shared_prefixes(base);
            self.cancel_layer_prefix(layer);
        }
    }

    /// Replaces a layer with runtime bindings, such as categories fetched from an API.
    pub fn set_layer(&mut self, layer: L, bindings: LayerKeymap<C>) {
        self.overrides.insert(layer, bindings);
        self.cancel_layer_prefix(layer);
    }

    /// Restores the layer's configured bindings.
    pub fn reset_layer(&mut self, layer: L) {
        if self.overrides.remove(&layer).is_some() {
            self.cancel_layer_prefix(layer);
        }
    }

    fn cancel_layer_prefix(&mut self, layer: L) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.layer == layer)
        {
            self.pending = None;
        }
    }

    fn map(&self, layer: L) -> Option<&LayerKeymap<C>> {
        self.overrides
            .get(&layer)
            .or_else(|| self.layers.get(&layer))
    }

    fn first(&self, stack: &LayerStack<L, I>, key: KeyStroke) -> Option<LayerOwner<L, I>> {
        stack.entries.iter().rev().copied().find(|entry| {
            self.map(entry.layer)
                .is_some_and(|map| map.takes_first(key))
        })
    }

    /// The topmost layer that reserves a first key, including prefixes, text
    /// fallbacks, and keys masked by a popup. Use `stack.owner(layer)` to check
    /// whether a command from that layer can be delivered.
    pub fn layer_for(&self, stack: &LayerStack<L, I>, key: KeyStroke) -> Option<L> {
        self.first(stack, key).map(|entry| entry.layer)
    }

    pub fn sync_layers(&mut self, stack: &LayerStack<L, I>) {
        if self.active_prefix(stack).is_none() {
            self.pending = None;
        }
    }

    fn active_prefix(&self, stack: &LayerStack<L, I>) -> Option<&Pending<L, I>> {
        self.pending.as_ref().filter(|p| {
            &p.stack == stack
                && self
                    .first(stack, p.keys[0])
                    .is_some_and(|entry| entry.layer == p.layer)
        })
    }

    pub fn pending_keys(&self) -> &[KeyStroke] {
        self.pending.as_ref().map_or(&[], |pending| &pending.keys)
    }

    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    pub fn resolve(&mut self, stack: &LayerStack<L, I>, key: KeyStroke) -> Resolution<L, I, C> {
        self.sync_layers(stack);
        let mut pending = self.pending.take();
        let keys = match &mut pending {
            Some(p) => {
                p.keys.push(key);
                p.keys.as_slice()
            }
            None => std::slice::from_ref(&key),
        };
        let Some(entry) = self.first(stack, keys[0]) else {
            return Resolution::Unbound;
        };
        let map = self
            .map(entry.layer)
            .expect("the selected layer has bindings");
        match map.lookup(keys) {
            Lookup::Match(_) if entry.covered => Resolution::Masked,
            Lookup::Match(command) => Resolution::Matched {
                layer: entry.layer,
                owner: entry.owner,
                command: command.clone(),
            },
            // The layer was chosen because it takes the first key. If that
            // single key has no explicit binding, the text fallback owns it.
            Lookup::Missing if keys.len() == 1 && entry.covered => Resolution::Masked,
            Lookup::Missing if keys.len() == 1 => {
                map.input_char(key)
                    .map_or(Resolution::Cancelled, |command| Resolution::Matched {
                        layer: entry.layer,
                        owner: entry.owner,
                        command,
                    })
            }
            Lookup::Missing => Resolution::Cancelled,
            Lookup::Prefix => {
                self.pending = Some(pending.unwrap_or_else(|| Pending {
                    layer: entry.layer,
                    stack: stack.clone(),
                    keys: vec![key],
                }));
                Resolution::Pending
            }
        }
    }

    pub fn hints(&self, stack: &LayerStack<L, I>) -> Vec<KeyHint<'_, L, C>> {
        let mut hints = Vec::new();
        let prefix = self.active_prefix(stack);
        let pending_keys: &[KeyStroke] = prefix.map_or(&[], |p| &p.keys);
        for entry in stack.entries.iter().rev().filter(|e| !e.covered) {
            let Some(map) = self.map(entry.layer) else {
                continue;
            };
            if prefix.is_some_and(|p| p.layer != entry.layer) {
                continue;
            }
            for binding in &map.bindings {
                let keys = binding.on.as_slice();
                let first = keys[0];
                if self.layer_for(stack, first) != Some(entry.layer) {
                    continue;
                }
                let Some(suffix) = keys.strip_prefix(pending_keys) else {
                    continue;
                };
                hints.push(KeyHint {
                    layer: entry.layer,
                    keys: suffix
                        .iter()
                        .map(|key| key.help_label())
                        .collect::<Vec<_>>()
                        .join(" "),
                    command: &binding.command,
                    description: binding.desc.as_deref(),
                });
            }
        }
        hints
    }
}

impl<U: Components, B: Clone + KeyAction<U>> KeyBindings<U> for Keymap<U::Layer, B, U::Id> {
    fn resolve_command(
        &mut self,
        layers: &LayerStack<U::Layer, U::Id>,
        key: KeyStroke,
    ) -> Option<(U::Id, U::Command)> {
        match self.resolve(layers, key) {
            Resolution::Matched { owner, command, .. } => Some(command.command_for(owner)),
            _ => None,
        }
    }

    fn cancel_pending(&mut self) {
        self.cancel_pending();
    }

    fn sync_layers(&mut self, layers: &LayerStack<U::Layer, U::Id>) {
        self.sync_layers(layers);
    }
}
