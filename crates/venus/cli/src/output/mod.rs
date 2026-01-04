//! Output handling for Venus CLI.
//!
//! This module provides utilities for decoding and formatting cell outputs
//! for terminal display.
//!
//! # Module Structure
//!
//! - `decoder` - Bincode decoding for common Rust types
//! - `formatter` - Terminal formatting and pretty-printing

pub mod decoder;
mod formatter;

pub use formatter::print_output;
