//! Procedural macros for Venus reactive notebook environment.
//!
//! This crate provides the `#[venus::cell]` attribute macro that marks functions
//! as notebook cells. The macro is a passthrough in library mode (for `cargo build`),
//! while the Venus runtime interprets these attributes for reactive execution.

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemFn, parse_macro_input};

/// Marks a function as a notebook cell.
///
/// # Cell Semantics
///
/// - **Dependencies**: Function parameters define inputs from other cells
/// - **Output**: Return type defines what this cell provides to dependents
/// - **Documentation**: `///` comments become cell descriptions in the notebook UI
///
/// # Example
///
/// ```rust,ignore
/// use venus::prelude::*;
///
/// /// Load data from CSV file
/// #[venus::cell]
/// pub fn load_data() -> DataFrame {
///     CsvReader::from_path("data.csv")
///         .unwrap()
///         .finish()
///         .unwrap()
/// }
///
/// /// Process the loaded data
/// #[venus::cell]
/// pub fn process(data: &DataFrame) -> DataFrame {
///     data.clone()
///         .lazy()
///         .filter(col("value").gt(lit(100)))
///         .collect()
///         .unwrap()
/// }
/// ```
///
/// # Behavior
///
/// In **library mode** (when compiled with `cargo build`), this attribute is a
/// passthrough - the function is compiled as-is. This ensures full compatibility
/// with rust-analyzer, cargo, and clippy.
///
/// In **Venus runtime mode**, the attribute signals to the execution engine that
/// this function should be treated as a reactive cell with automatic dependency
/// tracking and re-execution.
#[proc_macro_attribute]
pub fn cell(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    // Parse optional attributes (future use: priority, cache settings, etc.)
    let _attr_tokens = proc_macro2::TokenStream::from(attr);

    // For now, passthrough the function unchanged.
    // The Venus runtime will parse the source file with `syn` to extract
    // cell metadata (dependencies, return type, doc comments).
    //
    // We add a hidden marker attribute that the runtime can detect,
    // but this doesn't affect compilation.
    let vis = &input.vis;
    let sig = &input.sig;
    let block = &input.block;
    let attrs = &input.attrs;

    let expanded = quote! {
        #(#attrs)*
        #[doc(hidden)]
        #[allow(dead_code)]
        const _: () = {
            // Marker for Venus runtime detection
            // This const is optimized away but appears in the AST
        };
        #vis #sig #block
    };

    TokenStream::from(expanded)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_cell_macro_compiles() {
        // This test just verifies the macro crate compiles
        // Actual macro testing requires a separate test crate
    }
}
