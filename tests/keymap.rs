use serde::Deserialize;
use tuiframework::keymap::{
    Binding, KeySequence, KeyStroke, Keymap, KeymapConfig, KeymapError, LayerKeymap, LayerStack,
    Resolution,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Layer {
    Global,
    List,
    Files,
    Prompt,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Command {
    Quit,
    First,
    Last,
    Open,
    Filter,
    Text(char),
}

type Map = Keymap<Layer, Command, u8>;

fn key(notation: &str) -> KeyStroke {
    notation.parse().unwrap()
}

fn binding(keys: &str, command: Command) -> Binding<Command> {
    Binding {
        on: keys.parse().unwrap(),
        command,
        desc: None,
    }
}

const DEFAULTS: &str = r#"
[global]
keymap = [{on = "q", command = "quit"}]
[list]
keymap = [{on = ["g", "g"], command = "first"}, {on = ["g", "e"], command = "last"}]
[files]
keymap = [{on = ["u", "n"], command = "filter", desc = "Filter"}]
"#;

#[test]
fn configured_alias_precedes_defaults_even_when_a_sibling_shares_the_first_key() {
    let defaults: KeymapConfig<Layer, Command> = toml::from_str(DEFAULTS).unwrap();
    let user = toml::from_str(
        r#"
        [files]
        keymap = [{on = ["u", "x"], command = "open"}, {on = "y", command = "filter"}]
    "#,
    )
    .unwrap();
    let mut map = Map::compile(defaults.with_overrides(user).unwrap()).unwrap();
    let mut layers = LayerStack::default();
    layers.extend(1, &[Layer::Files]);
    let hints = map.hints(&layers);
    assert_eq!(
        hints
            .iter()
            .find(|hint| *hint.command == Command::Filter)
            .unwrap()
            .keys,
        "y"
    );
    assert_eq!(map.resolve(&layers, key("u")), Resolution::Pending);
    assert_eq!(
        map.hints(&layers)
            .iter()
            .map(|h| h.keys.as_str())
            .collect::<Vec<_>>(),
        ["x", "n"]
    );
    assert_eq!(
        map.resolve(&layers, key("n")),
        Resolution::Matched {
            owner: 1,
            command: Command::Filter
        }
    );
}

#[test]
fn local_prefix_inherits_siblings_and_a_change_of_owner_cancels_it() {
    let mut defaults: KeymapConfig<Layer, Command> = toml::from_str(DEFAULTS).unwrap();
    defaults.bind(Layer::Files, binding("g g", Command::Open));
    let mut map = Map::compile(defaults).unwrap();
    map.inherit_shared_prefixes(Layer::Files, Layer::List);
    let mut layers = LayerStack::default();
    layers.extend(1, &[Layer::List, Layer::Files]);
    for (last, command) in [("g", Command::Open), ("e", Command::Last)] {
        assert_eq!(map.resolve(&layers, key("g")), Resolution::Pending);
        assert_eq!(
            map.resolve(&layers, key(last)),
            Resolution::Matched { owner: 1, command }
        );
    }
    assert_eq!(map.resolve(&layers, key("g")), Resolution::Pending);
    layers.extend(2, &[Layer::Files]);
    assert_eq!(map.resolve(&layers, key("e")), Resolution::Unbound);
    assert_eq!(map.pending_keys(), &[]);
    assert_eq!(map.resolve(&layers, key("g")), Resolution::Pending);
    assert_eq!(map.resolve(&layers, key("x")), Resolution::Cancelled);
}

#[test]
fn masked_layers_consume_keys_and_runtime_text_layers_shadow_the_background() {
    let mut map = Map::compile(toml::from_str(DEFAULTS).unwrap()).unwrap();
    let mut layers = LayerStack::default();
    layers.extend(1, &[Layer::Global, Layer::List, Layer::Files]);
    layers.cover(&[Layer::Global]);
    assert_eq!(map.resolve(&layers, key("q")), Resolution::Masked);
    assert!(
        map.hints(&layers)
            .iter()
            .all(|h| *h.command != Command::Quit)
    );
    layers.extend(2, &[Layer::Prompt]);
    assert_eq!(map.resolve(&layers, key("g")), Resolution::Pending);
    let mut prompt = LayerKeymap::default();
    prompt.bind_input_chars(Command::Text);
    prompt.bind(binding("C-c", Command::Quit)).unwrap();
    map.set_layer(Layer::Prompt, prompt);
    assert_eq!(map.hints(&layers).len(), 1);
    assert_eq!(
        map.resolve(&layers, key("e")),
        Resolution::Matched {
            owner: 2,
            command: Command::Text('e')
        }
    );
    assert_eq!(
        map.resolve(&layers, key("C-c")),
        Resolution::Matched {
            owner: 2,
            command: Command::Quit
        }
    );
    map.reset_layer(Layer::Prompt);
    layers.truncate_after(Layer::Global);
    assert_eq!(layers.top(), Some(Layer::Global));
    assert_eq!(map.resolve(&layers, key("g")), Resolution::Unbound);
}

#[test]
fn config_rejects_ambiguous_prefixes_and_equivalent_keys() {
    for (first, second) in [("g", "g g"), ("g g", "g")] {
        let mut map = LayerKeymap::default();
        map.bind(binding(first, Command::First)).unwrap();
        assert!(matches!(
            map.bind(binding(second, Command::Last)),
            Err(KeymapError::Ambiguous(_))
        ));
    }
    let user = toml::from_str(
        r#"[list]
        keymap = [{on = "G", command = "first"}, {on = "S-g", command = "last"}]"#,
    )
    .unwrap();
    assert!(matches!(
        KeymapConfig::<Layer, Command>::default().with_overrides(user),
        Err(KeymapError::Duplicate(_))
    ));
    assert!(matches!(
        "".parse::<KeySequence>(),
        Err(KeymapError::EmptyKeySequence)
    ));
    assert!("X-j".parse::<KeyStroke>().is_err());
    assert!(
        toml::from_str::<KeymapConfig<Layer, Command>>(
            r#"[list]
        keymap = [{on = [], command = "first"}]"#
        )
        .is_err()
    );
}

#[test]
fn config_can_validate_command_ids_before_compiling_them() {
    let config: KeymapConfig<Layer, Command> = toml::from_str(DEFAULTS).unwrap();
    let invalid = config.try_map_commands(|layer, command| {
        if *layer == Layer::Files && command == Command::Filter {
            Err("filter unavailable")
        } else {
            Ok(command)
        }
    });
    assert_eq!(invalid.unwrap_err(), "filter unavailable");
}
