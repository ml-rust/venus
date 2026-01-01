//! Compilation pipeline for Venus notebooks.
//!
//! This module provides:
//! - Toolchain management (Cranelift nightly installation)
//! - Universe building (dependency crate compilation)
//! - Cell compilation (individual cell → dylib)
//! - Error mapping (rustc errors → source locations)
//! - Dependency parsing (cargo-style specs from doc comments)
//!
//! # Architecture
//!
//! ```text
//! Notebook (.rs)
//!     │
//!     ├── Dependencies Block ──► DependencyParser ──► Universe Builder ──► libvenus_universe.so
//!     │
//!     └── Cell Functions ──► Cell Compiler ──► cell_*.so (Cranelift, fast)
//!                                   │
//!                                   └── Links against Universe
//! ```

mod cargo_generator;
mod cell;
mod dependency_parser;
mod errors;
mod production;
mod source_processor;
mod toolchain;
mod types;
mod universe;

pub use cargo_generator::{generate_cargo_toml, ManifestConfig, ReleaseProfile};
pub use cell::CellCompiler;
pub use dependency_parser::{DependencyParser, ExternalDependency};
pub use errors::{CompileError, ErrorMapper};
pub use production::ProductionBuilder;
pub use source_processor::NotebookSourceProcessor;
pub use toolchain::ToolchainManager;
pub use types::{CompilationResult, CompiledCell, CompilerConfig};
pub use universe::UniverseBuilder;
