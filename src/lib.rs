//! An event-driven Ratatui application with typed operations, drivers and keymaps.
//!
//! Implement [`Ui`] for the application's screen state and [`Driver`] for its
//! external operations. Supply them and the terminal I/O to [`EventLoop::builder`].
//!
//! [Getting started](https://github.com/ymgyt/tuiframework/blob/main/docs/getting_started.md)
//! builds a clock from an empty Cargo project.

mod component;
mod compositor;
pub mod driver;
pub mod event_loop;
pub mod keymap;
mod terminal;
pub mod theme;
mod ui;

pub use component::{Component, Effects, RenderContext};
pub use compositor::{Compositor, DispatchResult, TerminalFocus};
pub use driver::{Driver, DriverContext};
pub use event_loop::{AnimationTimer, EventLoop, EventLoopBuilder, Hooks, RunError};
pub use keymap::{KeyAction, UiKeymap};
pub use terminal::{RenderTarget, Terminal, TerminalDevice, TerminalOptions};
pub use ui::Ui;
pub use {crossterm, ratatui};
