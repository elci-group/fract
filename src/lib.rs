//! Fract — autonomous architectural maintenance daemon.
//!
//! Fract continuously observes a software project, scores structural entropy,
//! proposes semantic refactorings, validates them, and applies them safely.

pub mod complexity;
pub mod confidence;
pub mod config;
pub mod daemon;
pub mod engine_http;
pub mod events;
pub mod git;
pub mod indexer;
pub mod merge;
pub mod pr;
pub mod prompt;
pub mod queue;
pub mod refactor;
pub mod report;
pub mod shatter;
pub mod validation;
pub mod web;

// Internal zero-dependency replacements for third-party crates.
pub mod cli;
pub mod error;
pub mod id;
pub mod json;
pub mod scanner;
pub mod scratch;
pub mod store;
pub mod time;
pub mod walk;

mod model;
pub use model::*;
