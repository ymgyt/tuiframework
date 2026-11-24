use std::{collections::HashSet, hash::Hash};

use indexmap::IndexMap;
use serde::Deserialize;

use super::{KeySequence, KeymapError};

/// One key sequence and the application command it invokes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding<C> {
    pub on: KeySequence,
    pub command: C,
    pub desc: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "C: Deserialize<'de>"))]
pub(super) struct LayerConfig<C> {
    #[serde(default)]
    pub(super) keymap: Vec<Binding<C>>,
}

/// Uncompiled bindings, grouped by application-defined layer.
/// Each `[layer]` table contains a `keymap` array.
#[derive(Clone, Debug, Deserialize)]
#[serde(
    transparent,
    bound(deserialize = "L: Deserialize<'de> + Hash + Eq, C: Deserialize<'de>")
)]
pub struct KeymapConfig<L, C> {
    pub(super) layers: IndexMap<L, LayerConfig<C>>,
}

impl<L, C> Default for KeymapConfig<L, C> {
    fn default() -> Self {
        Self {
            layers: IndexMap::new(),
        }
    }
}

impl<L: Hash + Eq, C> KeymapConfig<L, C> {
    /// Validates commands in their layer and converts configuration IDs into
    /// the commands delivered to components.
    pub fn try_map_commands<T, E>(
        self,
        mut map: impl FnMut(&L, C) -> Result<T, E>,
    ) -> Result<KeymapConfig<L, T>, E> {
        let mut result = KeymapConfig::default();
        for (layer, config) in self.layers {
            let mut bindings = Vec::with_capacity(config.keymap.len());
            for Binding { on, command, desc } in config.keymap {
                bindings.push(Binding {
                    on,
                    command: map(&layer, command)?,
                    desc,
                });
            }
            result
                .layers
                .insert(layer, LayerConfig { keymap: bindings });
        }
        Ok(result)
    }

    pub fn bind(&mut self, layer: L, binding: Binding<C>) {
        self.layers
            .entry(layer)
            .or_insert_with(|| LayerConfig { keymap: Vec::new() })
            .keymap
            .push(binding);
    }

    /// Replaces matching sequences. User bindings precede built-in aliases in help.
    pub fn with_overrides(mut self, user: Self) -> Result<Self, KeymapError> {
        for (layer, mut overrides) in user.layers {
            let mut seen = HashSet::new();
            for binding in &overrides.keymap {
                if !seen.insert(&binding.on) {
                    return Err(KeymapError::Duplicate(binding.on.clone()));
                }
            }
            if let Some(defaults) = self.layers.get_mut(&layer) {
                defaults
                    .keymap
                    .retain(|binding| !seen.contains(&binding.on));
                overrides.keymap.append(&mut defaults.keymap);
            }
            self.layers.insert(layer, overrides);
        }
        Ok(self)
    }
}
