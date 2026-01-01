//! State management for Venus notebooks.
//!
//! This module provides:
//! - Cell output serialization (Serde + rkyv fast path)
//! - Schema evolution detection
//! - State persistence and restoration

mod manager;
mod output;
mod schema;

pub use manager::StateManager;
pub use output::{BoxedOutput, CellOutput, ZeroCopyOutput};
pub use schema::{SchemaChange, TypeFingerprint};
