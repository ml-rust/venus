//! Cargo dependency context for generated notebook projects.

mod context;
mod exports;
mod feature_graph;
mod render;
mod resolve;
mod runtime;
mod spec;
#[cfg(test)]
mod tests;

pub(super) use context::NotebookContext;
pub use context::find_notebook_manifest;
pub(super) use resolve::ResolvedManifest;
pub(super) use spec::notebook_document;
