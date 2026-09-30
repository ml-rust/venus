//! Shared dependency library for notebook cells.

mod builder;
mod generation;
#[cfg(test)]
mod tests;

pub use builder::UniverseBuilder;
