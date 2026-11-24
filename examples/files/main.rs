use std::{env, error::Error, io, path::PathBuf, sync::Arc};

use serde::Deserialize;
use tuiframework::{
    AppKeymap, Application, Component, Components, Driver, DriverContext, Effects,
    keymap::KeymapConfig,
    ratatui::style::Style,
    theme::{Palette, Styles},
};

mod listing;
mod sort_menu;
#[cfg(test)]
mod tests;

use listing::Listing;
use sort_menu::{Order, SortMenu};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Id {
    Files,
    Sort,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
enum Layer {
    Global,
    List,
    Files,
    Sort,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Command {
    Quit,
    Next,
    Previous,
    First,
    Reload,
    OpenSort,
    Close,
    Choose,
    ToggleTheme,
}

enum Event {
    Loaded(io::Result<Arc<[String]>>),
    Sorted(Order),
}

enum Operation {
    ReadDirectory(PathBuf),
}

/// The file browser uses a separate style for a directory read in progress.
struct FileTheme {
    common: Styles,
    loading: Style,
}

impl From<Palette> for FileTheme {
    fn from(palette: Palette) -> Self {
        let common = Styles::from(palette);
        Self {
            loading: common.base.fg(palette.accent),
            common,
        }
    }
}

/// Both components keep their own selection when the sort menu opens.
struct Files {
    listing: Listing,
    sort: SortMenu,
    theme: FileTheme,
}

impl Files {
    fn new(path: PathBuf, theme: FileTheme) -> Self {
        Self {
            listing: Listing::new(path),
            sort: SortMenu::default(),
            theme,
        }
    }
}

impl Components for Files {
    type Command = Command;
    type Keymap = AppKeymap<Self>;
    type Event = Event;
    type Operation = Operation;
    type Theme = FileTheme;
    fn theme(&self) -> &FileTheme {
        &self.theme
    }
    type View<'a> = tuiframework::RenderContext<'a, Self>;
    type Layer = Layer;
    type Id = Id;
    fn ids(&self) -> impl IntoIterator<Item = Id> {
        [Id::Files, Id::Sort]
    }

    fn component(&self, id: Id) -> Option<&dyn Component<Self>> {
        Some(match id {
            Id::Files => &self.listing,
            Id::Sort => &self.sort,
        })
    }

    fn component_mut(&mut self, id: Id) -> Option<&mut dyn Component<Self>> {
        Some(match id {
            Id::Files => &mut self.listing,
            Id::Sort => &mut self.sort,
        })
    }

    fn command(&mut self, to: Id, command: Command) -> Effects<Self> {
        if let Command::ToggleTheme = command {
            self.theme = if self.theme.common.palette == Palette::dracula() {
                Palette::ferra().into()
            } else {
                Palette::dracula().into()
            };
            return Effects::NONE;
        }
        self.component_mut(to)
            .map_or(Effects::NONE, |component| component.command(command))
    }

    fn visible(&self) -> impl IntoIterator<Item = Id> {
        std::iter::once(Id::Files).chain(self.sort.open.then_some(Id::Sort))
    }

    fn start(&mut self) -> Effects<Self> {
        Effects::NONE.send(Id::Files, Command::Reload)
    }

    fn is_busy(&self) -> bool {
        self.listing.loading
    }
    fn tick(&mut self) {
        self.listing.tick();
    }
}

struct FileSystem;

impl Driver<Files> for FileSystem {
    fn dispatch(
        &mut self,
        Operation::ReadDirectory(path): Operation,
        cx: &mut DriverContext<Event>,
    ) {
        cx.submit(async move { Event::Loaded(Self::read_directory(path).await) });
    }
}

impl FileSystem {
    async fn read_directory(path: PathBuf) -> io::Result<Arc<[String]>> {
        let mut dir = tokio::fs::read_dir(path).await?;
        let mut names = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort_unstable();
        Ok(names.into())
    }
}

fn keymap() -> Result<AppKeymap<Files>, Box<dyn Error>> {
    let config: KeymapConfig<Layer, Command> = toml::from_str(include_str!("keys.toml"))?;
    Ok(AppKeymap::<Files>::compile(config)?)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    Application::builder()
        .components(Files::new(path, Palette::default().into()))
        .driver(FileSystem)
        .keymap(keymap()?)
        .build()
        .run()
        .await?;
    Ok(())
}
