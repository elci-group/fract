//! Deterministic Rust source transformation engine.
//!
//! `shatter` converts Fract's architectural factorisation findings into deterministic
//! source transformations. It operates through a finite set of legal moves on a typed
//! AST representation, validated by the Rust compiler.

pub mod context;
pub mod executor;
pub mod preconditions;

pub use context::ShatterContext;
pub use executor::{execute_shatter, ShatterReport};
pub use preconditions::{CandidateFunction, PreconditionFailure, validate_candidate};
