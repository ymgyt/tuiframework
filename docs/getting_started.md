# Getting started

Build a clock that displays Unix time. Press `r` to read the time again and `q` to quit.

Create a binary crate.

```sh
cargo new tui-clock
cd tui-clock
```

Replace the empty `[dependencies]` section in `Cargo.toml` with these entries. Cargo records the framework's Git revision in `Cargo.lock`.

```toml
[dependencies]
tuiframework = { git = "https://github.com/ymgyt/tuiframework" }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt"] }
toml = "1"
```

Replace the generated `src/main.rs` with the five Rust blocks below, in order.

## 1. Define the screen and its messages

`ClockScreen` holds the last reading. Give each direction of communication its own type. A key produces a `Command`, the screen requests an `Operation`, and the driver answers with an `Event`.

```rust
use std::{error::Error, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

use serde::Deserialize;
use tuiframework::{
    Component, Compositor, Driver, DriverContext, Effects, EventLoop, Hooks, RenderContext,
    Terminal, TerminalDevice, TerminalOptions, Ui, UiKeymap,
    crossterm::event::EventStream,
    keymap::KeymapConfig,
    ratatui::{Frame, style::{Color, Style}, widgets::Paragraph},
    theme::Styles,
};

#[derive(Default)]
struct ClockScreen {
    seconds: Option<u64>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Command { Refresh, Quit }

enum Operation { ReadClock }
enum Event { Read(u64) }
```

## 2. Define the theme and register the screen

`Clock` owns the screens and their shared theme. The clock adds a style for its time reading to the common styles. Its theme stays in the application so a settings change can replace it between frames. The single screen uses `()` as its address. The named `Clock` key layer also identifies its table in the settings file. The compositor draws the IDs returned by `visible` from back to front. It visits `ids` when publishing events, so a hidden screen can also receive a result.

```rust
struct ClockTheme {
    common: Styles,
    reading: Style,
}

struct Clock {
    screen: ClockScreen,
    theme: ClockTheme,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Layer { Clock }

impl Ui for Clock {
    type Id = ();
    type Layer = Layer;
    type Command = Command;
    type Keymap = UiKeymap<Self>;
    type Event = Event;
    type Operation = Operation;
    type Theme = ClockTheme;
    type Context<'a> = RenderContext<'a, Self>;

    fn theme(&self) -> &ClockTheme { &self.theme }
    fn ids(&self) -> impl IntoIterator<Item = ()> { [()] }
    fn component(&self, _id: ()) -> Option<&dyn Component<Self>> { Some(&self.screen) }
    fn component_mut(&mut self, _id: ()) -> Option<&mut dyn Component<Self>> {
        Some(&mut self.screen)
    }
    fn visible(&self) -> impl IntoIterator<Item = ()> { [()] }
}

```

## 3. Handle commands and draw the result

The screen declares which key layers it owns. Pressing a bound key calls its `command` method. The screen asks the driver to read the clock, then replaces its displayed value when the event arrives.

```rust
impl Component<Clock> for ClockScreen {
    fn key_layers(&self) -> &[Layer] { &[Layer::Clock] }

    fn apply_command(&mut self, command: Command) -> Effects<Clock> {
        match command {
            Command::Refresh => Effects::NONE.operation(Operation::ReadClock),
            Command::Quit => Effects::NONE.quit(),
        }
    }

    fn subscribes(&self, event: &Event) -> bool { matches!(event, Event::Read(_)) }

    fn apply_event(&mut self, Event::Read(seconds): &Event) -> Effects<Clock> {
        self.seconds = Some(*seconds);
        Effects::NONE
    }

    fn render(&self, frame: &mut Frame<'_>, cx: &RenderContext<'_, Clock>) {
        let (text, style) = match self.seconds {
            Some(s) => (format!("Unix time {s}"), cx.theme.reading),
            None => ("Reading…".into(), cx.theme.common.muted),
        };
        frame.render_widget(Paragraph::new(text).style(style), cx.area);
    }
}
```

## 4. Implement the driver

Reading the system clock is synchronous. The driver uses `emit` to queue the result for the screen. For asynchronous I/O, use `submit` with a future that returns an event. A test driver can return a fixed reading through the same interface.

```rust
struct SystemClock;

impl Driver for SystemClock {
    type Event = Event;
    type Operation = Operation;

    fn dispatch(&mut self, operation: Operation, cx: &mut DriverContext<Event>) {
        match operation {
            Operation::ReadClock => {
                let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
                cx.emit(Event::Read(elapsed.as_secs()));
            }
        }
    }
}
```

## 5. Resolve settings and start the application

The application decides where its settings come from. Here it takes an optional TOML file on the command line. The file can set the reading's color and override individual keys. The built-in keymap has a `[clock]` table for the screen. In the settings file, that same table goes under `[keys.clock]`.

`with_overrides` merges bindings by layer and key sequence. `compile` checks that a complete binding is never also the prefix of a longer one. Both run before the application opens the terminal.

```rust
const DEFAULT_KEYS: &str = r#"
[clock]
keymap = [
    { on = "r", command = "refresh" },
    { on = "q", command = "quit" },
]
"#;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default)]
    keys: KeymapConfig<Layer, Command>,
    reading_color: Option<String>,
}

fn settings(path: Option<PathBuf>) -> Result<(ClockTheme, UiKeymap<Clock>), Box<dyn Error>> {
    let user: Settings = match path {
        Some(path) => toml::from_str(&std::fs::read_to_string(path)?)?,
        None => Settings::default(),
    };
    let defaults: KeymapConfig<Layer, Command> = toml::from_str(DEFAULT_KEYS)?;
    let keys = UiKeymap::<Clock>::compile(defaults.with_overrides(user.keys)?)?;
    let common = Styles::default();
    let color = match user.reading_color {
        Some(name) => name.parse::<Color>()?,
        None => common.palette.accent,
    };
    let reading = common.base.fg(color);
    Ok((ClockTheme { common, reading }, keys))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let (theme, keys) = settings(path)?;
    let clock = Clock { screen: ClockScreen::default(), theme };
    let hooks = Hooks::default().on_start(Box::new(|_, _| Effects::NONE.send((), Command::Refresh)));
    let mut device = TerminalDevice::stdout();
    device.install_abort_hook();
    EventLoop::builder()
        .compositor(Compositor::new(clock, keys))
        .driver(SystemClock)
        .terminal(Terminal::init(&mut device, TerminalOptions::default())?)
        .input(EventStream::new())
        .hooks(hooks)
        .build()
        .run()
        .await?;
    Ok(())
}
```

Run the clock. It reads the time once at startup. After a second, press `r` to see the value change.

```sh
cargo run
```

To try a user setting, save the following as `settings.toml`. It displays the time in cyan, makes `F5` refresh the screen and rebinds `r` to quit. The default `q` still quits.

```toml
reading_color = "cyan"

[keys.clock]
keymap = [
    { on = "f5", command = "refresh" },
    { on = "r", command = "quit" },
]
```

```sh
cargo run -- settings.toml
```

## Add a second screen

Give each screen an ID and return the open screens from `visible`. When two screens declare the same layer, its bound command goes to the frontmost one. To see this in the file browser, run the following command from the framework checkout.

```sh
cargo run --example files -- .
```

Move down the file list with `j`, open the sort menu with `s`, then press `j` again. This time the selection moves in the menu. Press Escape to return to the list with its selection intact. Press `t` to change the theme. The example's headless tests send the same keys and inspect the terminal buffer. [File browser](../examples/files/main.rs) · [Headless tests](../examples/files/tests.rs)
