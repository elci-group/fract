//! Deterministic Rust source transformation engine.
//!
//! `shatter` converts Fract's architectural factorisation findings into deterministic
//! source transformations. It operates through a finite set of legal moves on a typed
//! AST representation, validated by the Rust compiler.

pub mod ast_rewrite;
pub mod context;
pub mod executor;
pub mod graph;
pub mod moves;
pub mod preconditions;
pub mod transactional;

pub use ast_rewrite::AstRewriter;
pub use context::ShatterContext;
pub use executor::{execute_shatter, ShatterReport};
pub use graph::{DependencyGraph, FunctionId};
pub use moves::{Move, MoveSequence, PublicVisibility};
pub use preconditions::{CandidateFunction, PreconditionFailure, validate_candidate};
pub use transactional::{Transaction, FileChange, ValidationResult};
