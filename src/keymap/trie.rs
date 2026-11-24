use std::{collections::HashMap, fmt, sync::Arc};

use super::{Binding, KeyStroke, KeymapError};

/// Compiled bindings for one layer. Help retains declaration order, including aliases.
#[derive(Clone)]
pub struct LayerKeymap<C> {
    root: Node,
    pub(super) bindings: Vec<Binding<C>>,
    input_chars: Option<Arc<dyn Fn(char) -> C + Send + Sync>>,
}

impl<C: fmt::Debug> fmt::Debug for LayerKeymap<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayerKeymap")
            .field("bindings", &self.bindings)
            .field("accepts_text", &self.input_chars.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default)]
struct Node {
    binding: Option<usize>,
    children: HashMap<KeyStroke, Self>,
}

pub(super) enum Lookup<'a, C> {
    Match(&'a C),
    Prefix,
    Missing,
}

impl<C> Default for LayerKeymap<C> {
    fn default() -> Self {
        Self {
            root: Node::default(),
            bindings: Vec::new(),
            input_chars: None,
        }
    }
}

impl<C> LayerKeymap<C> {
    pub fn bind(&mut self, binding: Binding<C>) -> Result<(), KeymapError> {
        let mut node = &mut self.root;
        for key in binding.on.as_slice() {
            if node.binding.is_some() {
                return Err(KeymapError::Ambiguous(binding.on));
            }
            node = node.children.entry(*key).or_default();
        }
        if node.binding.is_some() {
            return Err(KeymapError::Duplicate(binding.on));
        }
        if !node.children.is_empty() {
            return Err(KeymapError::Ambiguous(binding.on));
        }
        node.binding = Some(self.bindings.len());
        self.bindings.push(binding);
        Ok(())
    }

    /// Maps a typed character left over after checking this layer's explicit keys
    /// and prefixes. A fallback in a higher layer still shadows lower bindings.
    pub fn bind_input_chars(&mut self, command: impl Fn(char) -> C + Send + Sync + 'static) {
        self.input_chars = Some(Arc::new(command));
    }

    pub(super) fn input_char(&self, key: KeyStroke) -> Option<C> {
        let ch = key.as_char()?;
        self.input_chars.as_ref().map(|command| command(ch))
    }

    pub(super) fn takes_first(&self, key: KeyStroke) -> bool {
        self.root.children.contains_key(&key)
            || self.input_chars.is_some() && key.as_char().is_some()
    }

    pub(super) fn lookup(&self, keys: &[KeyStroke]) -> Lookup<'_, C> {
        let mut node = &self.root;
        for key in keys {
            let Some(child) = node.children.get(key) else {
                return Lookup::Missing;
            };
            node = child;
        }
        match node.binding {
            Some(index) => Lookup::Match(&self.bindings[index].command),
            None => Lookup::Prefix,
        }
    }

    pub(super) fn inherit_shared_prefixes(&mut self, shared: &Self)
    where
        C: Clone,
    {
        for binding in &shared.bindings {
            if self.root.children.contains_key(&binding.on.as_slice()[0]) {
                // A local leaf or prefix wins any conflict with the shared layer.
                let _ = self.bind(binding.clone());
            }
        }
    }
}
