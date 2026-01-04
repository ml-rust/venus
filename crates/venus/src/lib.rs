//! Venus: A reactive notebook environment for Rust.
//!
//! Venus provides an interactive notebook experience with:
//! - **Reactive execution**: Cells automatically re-execute when dependencies change
//! - **Full IDE support**: Uses `.rs` files, so rust-analyzer works out of the box
//! - **Fast compilation**: Cranelift JIT for sub-second feedback
//! - **Hot reload**: Modify code without losing state
//!
//! # ✅ API Stability
//!
//! **This crate provides the STABLE user-facing API for Venus notebooks.**
//!
//! Starting from 0.1.0, the following APIs are considered **stable**:
//! - `#[venus::cell]` proc macro
//! - `venus::prelude::*` module
//! - `Render` trait and its derive macro
//! - Widget functions (`input_slider`, `input_text`, etc.)
//!
//! **Stability guarantees:**
//! - ✅ **SemVer compliance** - breaking changes only in major versions (1.0, 2.0, etc.)
//! - ✅ **Deprecation warnings** - minimum 1 minor version warning before removal
//! - ✅ **Documented migrations** - clear upgrade paths for breaking changes
//!
//! For internal APIs (compilation, execution, state management), see the `venus-core` crate,
//! which is **UNSTABLE** and may change without notice.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use venus::prelude::*;
//!
//! /// Load configuration
//! #[venus::cell]
//! pub fn config() -> Config {
//!     Config::default()
//! }
//!
//! /// Process data using config
//! #[venus::cell]
//! pub fn process(config: &Config) -> Result<Data, Error> {
//!     // config is automatically passed from the config() cell
//!     load_and_process(&config.path)
//! }
//! ```
//!
//! # Cell Dependencies
//!
//! Dependencies are inferred from function parameters:
//! - `fn foo(x: &T)` depends on a cell that returns `T`
//! - `fn bar(a: &A, b: &B)` depends on cells returning `A` and `B`
//!
//! The parameter name must match the producing cell's function name.

pub use venus_macros::cell;

pub mod render;
pub mod widgets;

pub mod prelude {
    //! Common imports for Venus notebooks.
    //!
    //! ```rust,ignore
    //! use venus::prelude::*;
    //! ```

    pub use crate::cell;
    pub use crate::render::Render;

    // Re-export rkyv derives for user structs (all cell return types need serialization)
    pub use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

    // Widget functions
    pub use crate::widgets::{
        input_checkbox, input_checkbox_labeled,
        input_select, input_select_labeled,
        input_slider, input_slider_labeled, input_slider_with_step,
        input_text, input_text_labeled, input_text_with_default,
    };
}

/// Re-export for convenience
pub use render::Render;

// Re-export widget functions at crate root for convenience
pub use widgets::{
    input_checkbox, input_checkbox_labeled,
    input_select, input_select_labeled,
    input_slider, input_slider_labeled, input_slider_with_step,
    input_text, input_text_labeled, input_text_with_default,
};
