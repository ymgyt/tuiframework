//! An event-driven Ratatui application with typed operations, drivers and keymaps.
//!
//! Implement [`Components`] for the component store and [`Driver`] for its
//! external operations. [`Application::run`] connects them to a fullscreen terminal.

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
pub use terminal::TerminalSession;
pub use {crossterm, ratatui};
