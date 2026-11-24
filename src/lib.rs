//! An event-driven Ratatui application with typed operations, drivers and keymaps.
//!
//! Implement [`Components`] for the component store and [`Driver`] for its
//! external operations. [`Application::run`] connects them to a fullscreen terminal.
//!
//! [Getting started](https://github.com/ymgyt/tuiframework/blob/main/docs/getting_started.md)
//! builds a clock from an empty Cargo project.

mod application;
mod component;
mod compositor;
pub mod driver;
pub mod keymap;
mod terminal;
pub mod theme;

pub use application::{Application, RunError};
pub use component::{AppKeymap, Component, Components, Effects, KeyAction, RenderContext};
pub use compositor::Compositor;
pub use driver::{Driver, DriverContext};
pub use terminal::{TerminalOptions, TerminalSession};
pub use {crossterm, ratatui};
