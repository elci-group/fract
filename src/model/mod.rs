//! Core domain model shared across the crate.

mod event;
mod health;
mod module;
mod proposal;

pub use event::*;
pub use health::*;
pub use module::*;
pub use proposal::*;
