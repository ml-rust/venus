//! Terminal color constants and utilities for CLI output.

use std::io::{self, Write};

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const CYAN: &str = "\x1b[36m";
pub const RED: &str = "\x1b[31m";

/// Flush stdout to ensure progress output is visible immediately.
///
/// This is useful when printing progress indicators without a trailing newline.
#[inline]
pub fn flush_stdout() {
    io::stdout().flush().ok();
}
