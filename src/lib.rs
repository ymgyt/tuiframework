//! An event-driven Ratatui application with typed operations, drivers and keymaps.
//!
//! Implement [`Components`] for the application's screen state and [`Driver`] for its
//! external operations. Supply them and the terminal I/O to [`EventLoop::builder`].
//!
//! [Getting started](https://github.com/ymgyt/tuiframework/blob/main/docs/getting_started.md)
//! builds a clock from an empty Cargo project.

mod component;
mod components;
mod compositor;
pub mod driver;
pub mod event_loop;
pub mod keymap;
mod overlay;
mod terminal;
pub mod theme;

pub use component::{ApplicationEffects, Component, Effects, RenderInputs};
pub use compositor::TerminalFocus;
/// Drives screen transitions without a terminal or external driver.
pub mod testing {
    pub use crate::compositor::{Compositor as TestScreen, DispatchResult};
}
pub use components::Components;
pub use driver::{Driver, DriverContext};
pub use event_loop::{AnimationTimer, EventLoop, EventLoopBuilder, Hooks, RunError};
pub use keymap::{KeyAction, UiKeymap};
pub use overlay::Overlay;
pub use terminal::{RenderTarget, Terminal, TerminalOptions};
pub use {crossterm, ratatui};
