use std::{env, error::Error, io, path::PathBuf, sync::Arc};

use serde::Deserialize;
use tuiframework::{
    AppKeymap, Application, Component, Components, Driver, DriverContext, Effects,
    keymap::KeymapConfig, theme::Theme,
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
}

enum Event {
    Loaded(io::Result<Arc<[String]>>),
    Sorted(Order),
}

enum Operation {
    ReadDirectory(PathBuf),
}

/// Both components keep their own selection when the sort menu opens.
struct Files {
    listing: Listing,
    sort: SortMenu,
    theme: Theme,
}

impl Files {
    fn new(path: PathBuf) -> Self {
        Self {
            listing: Listing::new(path),
            sort: SortMenu::default(),
            theme: Theme::default(),
        }
    }
}

impl Components for Files {
    type Command = Command;
    type Binding = Command;
    type Event = Event;
    type Operation = Operation;
    type View<'a> = tuiframework::RenderContext<'a, Self>;
    type Layer = Layer;
    type Id = Id;
    fn ids() -> impl IntoIterator<Item = Id> {
        [Id::Files, Id::Sort]
    }

    fn component(&self, id: Id) -> &dyn Component<Self> {
        match id {
            Id::Files => &self.listing,
            Id::Sort => &self.sort,
        }
    }

    fn component_mut(&mut self, id: Id) -> &mut dyn Component<Self> {
        match id {
            Id::Files => &mut self.listing,
            Id::Sort => &mut self.sort,
        }
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
    Application::new(Files::new(path), FileSystem, keymap()?)
        .run()
        .await?;
    Ok(())
}
