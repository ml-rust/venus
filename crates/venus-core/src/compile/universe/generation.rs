use crate::compile::ManifestConfig;
use crate::error::Result;

use super::UniverseBuilder;

impl UniverseBuilder {
    pub(super) fn generate_cargo_toml(&self) -> Result<String> {
        self.resolved.render(&ManifestConfig {
            name: "venus_universe",
            lib_crate_types: Some(&["cdylib", "rlib"]),
            standalone_workspace: true,
            ..ManifestConfig::default()
        })
    }

    /// Generate lib.rs that re-exports all dependencies and includes user types.
    pub(super) fn generate_lib_rs(&self) -> String {
        let mut lib = String::new();

        lib.push_str("//! Venus universe - re-exports all notebook dependencies.\n\n");

        // Allow common lints in generated code
        lib.push_str("#![allow(unused_imports)]\n");
        lib.push_str("#![allow(dead_code)]\n\n");

        // Always re-export rkyv for zero-copy cell serialization
        lib.push_str("pub use rkyv;\n");
        // Re-export derive macros for convenience
        lib.push_str("pub use rkyv::{Archive, Serialize as RkyvSerialize, Deserialize as RkyvDeserialize};\n");
        lib.push_str("pub use rkyv::rancor::Error as RkyvError;\n\n");

        // Re-export serde_json for widget JSON parsing in cell wrappers
        lib.push_str("pub use serde_json;\n\n");

        // Re-export venus widget functions and types for interactive notebooks
        lib.push_str(
            "pub use venus::{input_slider, input_slider_with_step, input_slider_labeled};\n",
        );
        lib.push_str("pub use venus::{input_text, input_text_with_default, input_text_labeled};\n");
        lib.push_str("pub use venus::{input_select, input_select_labeled};\n");
        lib.push_str("pub use venus::{input_checkbox, input_checkbox_labeled};\n");
        lib.push_str("pub use venus::widgets::{WidgetContext, WidgetValue, WidgetDef};\n");
        lib.push_str("pub use venus::widgets::{set_widget_context, take_widget_context};\n\n");

        lib.push_str(&self.resolved.reexports());

        // Re-export notebook imports so their names are visible to cells (which
        // link this crate and glob-import it via `use venus_universe::*;`).
        if !self.imports.is_empty() {
            lib.push_str("\n// Notebook imports (re-exported for cells)\n");
            lib.push_str(&self.imports);
            lib.push('\n');
        }

        // Include user-defined type definitions from the notebook
        if !self.type_definitions.is_empty() {
            lib.push_str("\n// User-defined types from notebook\n");
            lib.push_str(&self.type_definitions);
        }

        // Include notebook module for LSP analysis
        // This module is written by the server with current cell content
        lib.push_str("\n// Notebook cells (for LSP analysis)\n");
        lib.push_str("pub mod notebook;\n");

        lib
    }
}
