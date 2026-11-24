use std::{fmt, str::FromStr};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::KeymapError;

/// Key used for keymap lookup.
///
/// Equality and hashing normalize the case of `Char` keys the same way
/// crossterm's `KeyEvent` does: terminals report Shift+letter as an
/// uppercase char + SHIFT, while the keymap notation writes it as a
/// lowercase char + `S-` modifier. Comparing through the normalized form
/// makes both spellings identical.
#[derive(Clone, Copy, Debug)]
pub struct KeyStroke {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyStroke {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    /// Terminals can report either a base key plus Shift or its shifted key code.
    fn normalized(mut self) -> Self {
        if self.code == KeyCode::BackTab
            || (self.code == KeyCode::Tab && self.modifiers.contains(KeyModifiers::SHIFT))
        {
            self.code = KeyCode::BackTab;
            self.modifiers.insert(KeyModifiers::SHIFT);
            return self;
        }
        let KeyCode::Char(ch) = self.code else {
            return self;
        };
        if ch.is_ascii_uppercase() {
            self.modifiers.insert(KeyModifiers::SHIFT);
        } else if self.modifiers.contains(KeyModifiers::SHIFT) {
            self.code = KeyCode::Char(ch.to_ascii_uppercase());
        }
        self
    }

    pub const fn from_char(ch: char) -> Self {
        Self::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    pub fn help_label(self) -> String {
        let key = self.normalized();
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Enter) => "Ent".into(),
            (KeyModifiers::NONE, KeyCode::Char(' ')) => "Sp".into(),
            (KeyModifiers::NONE, KeyCode::Tab) => "Tab".into(),
            (KeyModifiers::SHIFT, KeyCode::BackTab) => "S-Tab".into(),
            _ => self.to_string(),
        }
    }

    pub fn as_char(self) -> Option<char> {
        if !(self.modifiers - KeyModifiers::SHIFT).is_empty() {
            return None;
        }
        match self.normalized().code {
            KeyCode::Char(ch) => Some(ch),
            _ => None,
        }
    }
}

impl PartialEq for KeyStroke {
    fn eq(&self, other: &Self) -> bool {
        let lhs = self.normalized();
        let rhs = other.normalized();
        lhs.code == rhs.code && lhs.modifiers == rhs.modifiers
    }
}

impl Eq for KeyStroke {}

impl std::hash::Hash for KeyStroke {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let normalized = self.normalized();
        normalized.code.hash(state);
        normalized.modifiers.hash(state);
    }
}

impl From<KeyEvent> for KeyStroke {
    fn from(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }
}

impl From<KeyStroke> for KeyEvent {
    fn from(key: KeyStroke) -> Self {
        Self::new(key.code, key.modifiers)
    }
}

impl FromStr for KeyStroke {
    type Err = KeymapError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_key_stroke(value)
            .map_err(|err| KeymapError::InvalidKeyNotation(value.to_owned(), err))
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(4);
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("C".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("A".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("S".to_owned());
        }

        for (modifier, name) in [
            (KeyModifiers::SUPER, "Super"),
            (KeyModifiers::HYPER, "Hyper"),
            (KeyModifiers::META, "Meta"),
        ] {
            if self.modifiers.contains(modifier) {
                parts.push(name.to_owned());
            }
        }

        let code = match self.code {
            KeyCode::Backspace => "backspace".to_owned(),
            KeyCode::Enter => "enter".to_owned(),
            KeyCode::Left => "left".to_owned(),
            KeyCode::Right => "right".to_owned(),
            KeyCode::Up => "up".to_owned(),
            KeyCode::Down => "down".to_owned(),
            KeyCode::Home => "home".to_owned(),
            KeyCode::End => "end".to_owned(),
            KeyCode::PageUp => "pageup".to_owned(),
            KeyCode::PageDown => "pagedown".to_owned(),
            KeyCode::Tab => "tab".to_owned(),
            KeyCode::BackTab => "backtab".to_owned(),
            KeyCode::Delete => "delete".to_owned(),
            KeyCode::Insert => "insert".to_owned(),
            KeyCode::F(n) => format!("f{n}"),
            KeyCode::Char(' ') => "space".to_owned(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Esc => "esc".to_owned(),
            KeyCode::Null => "null".to_owned(),
            KeyCode::CapsLock => "capslock".to_owned(),
            KeyCode::ScrollLock => "scrolllock".to_owned(),
            KeyCode::NumLock => "numlock".to_owned(),
            KeyCode::PrintScreen => "printscreen".to_owned(),
            KeyCode::Pause => "pause".to_owned(),
            KeyCode::Menu => "menu".to_owned(),
            KeyCode::KeypadBegin => "keypadbegin".to_owned(),
            KeyCode::Media(_) | KeyCode::Modifier(_) => format!("{:?}", self.code),
        };
        parts.push(code);
        f.write_str(&parts.join("-"))
    }
}

fn parse_key_stroke(value: &str) -> Result<KeyStroke, String> {
    let mut key = value;
    let mut modifiers = KeyModifiers::NONE;
    while key.len() > 1
        && let Some((modifier, rest)) = key.split_once('-')
    {
        modifiers.insert(match modifier {
            "C" => KeyModifiers::CONTROL,
            "S" => KeyModifiers::SHIFT,
            "A" => KeyModifiers::ALT,
            "Super" => KeyModifiers::SUPER,
            "Hyper" => KeyModifiers::HYPER,
            "Meta" => KeyModifiers::META,
            unknown => return Err(format!("unknown modifier '{unknown}'")),
        });
        key = rest;
    }
    let code = match key {
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "insert" => KeyCode::Insert,
        "delete" => KeyCode::Delete,
        "null" => KeyCode::Null,
        "capslock" => KeyCode::CapsLock,
        "scrolllock" => KeyCode::ScrollLock,
        "numlock" => KeyCode::NumLock,
        "printscreen" => KeyCode::PrintScreen,
        "pause" => KeyCode::Pause,
        "menu" => KeyCode::Menu,
        "keypadbegin" => KeyCode::KeypadBegin,
        "esc" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        single if single.chars().count() == 1 => KeyCode::Char(single.chars().next().unwrap()),
        function if function.starts_with('f') => {
            let number = function[1..]
                .parse::<u8>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("invalid function key '{function}'"))?;
            KeyCode::F(number)
        }
        unknown => return Err(format!("unknown key '{unknown}'")),
    };

    if code == KeyCode::BackTab {
        modifiers.insert(KeyModifiers::SHIFT);
    }
    Ok(KeyStroke::new(code, modifiers))
}

/// Ordered key strokes that must be typed to trigger one binding.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KeySequence {
    keys: Vec<KeyStroke>,
}

impl KeySequence {
    pub fn new(keys: Vec<KeyStroke>) -> Result<Self, KeymapError> {
        if keys.is_empty() {
            Err(KeymapError::EmptyKeySequence)
        } else {
            Ok(Self { keys })
        }
    }

    pub fn parse<const N: usize>(keys: [&str; N]) -> Result<Self, KeymapError> {
        let keys = keys
            .into_iter()
            .map(str::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(keys)
    }

    pub fn as_slice(&self) -> &[KeyStroke] {
        &self.keys
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys = self
            .keys
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        f.write_str(&keys)
    }
}

impl FromStr for KeySequence {
    type Err = KeymapError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(
            value
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?,
        )
    }
}

impl<'de> serde::Deserialize<'de> for KeySequence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Notation {
            One(String),
            Many(Vec<String>),
        }

        let keys = match Notation::deserialize(deserializer)? {
            Notation::One(key) => vec![key],
            Notation::Many(keys) => keys,
        };
        keys.iter()
            .map(|key| key.parse())
            .collect::<Result<_, _>>()
            .and_then(Self::new)
            .map_err(serde::de::Error::custom)
    }
}
