//! Output formatting for terminal display.
//!
//! Formats cell outputs for human-readable display in the terminal.

use crate::colors;

use super::decoder::try_decode_value;

/// Print a cell output in a readable format.
///
/// Attempts to decode the output based on the return type and displays it.
/// Falls back to hex dump for unknown types.
///
/// # Arguments
///
/// * `name` - Cell name
/// * `return_type` - The Rust return type as a string
/// * `bytes` - The serialized output bytes
pub fn print_output(name: &str, return_type: &str, bytes: &[u8]) {
    // Print header with cell name and type
    println!(
        "\n{}{}:{} {}{}{}",
        colors::CYAN,
        name,
        colors::RESET,
        colors::DIM,
        return_type,
        colors::RESET
    );

    // Try to interpret common types
    match try_decode_value(return_type, bytes) {
        Some(value) => {
            println!("  {}", value);
        }
        None => {
            // Provide informative message about unsupported type
            eprintln!(
                "  {}[Note]{} Type '{}' not directly displayable",
                colors::YELLOW,
                colors::RESET,
                return_type
            );
            // Fallback: show raw bytes as hex (truncated)
            let preview_len = bytes.len().min(64);
            println!(
                "  {}[{} bytes]{} {:?}{}",
                colors::DIM,
                bytes.len(),
                colors::RESET,
                &bytes[..preview_len],
                if bytes.len() > 64 { "..." } else { "" }
            );
        }
    }
}
