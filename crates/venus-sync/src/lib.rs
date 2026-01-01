//! Sync engine for Venus notebooks.
//!
//! Converts between `.rs` Venus notebooks and `.ipynb` Jupyter format.
//!
//! # Architecture
//!
//! ```text
//! notebook.rs ─────► RsParser ─────► CellsWithMeta ─────► IpynbGenerator ─────► notebook.ipynb
//!                                           │
//!                                           ▼
//!                                     OutputCache
//!                                    (.venus/outputs/)
//! ```

mod error;
mod ipynb;
mod outputs;
mod parser;

pub use error::{SyncError, SyncResult};
pub use ipynb::{IpynbGenerator, JupyterNotebook};
pub use outputs::OutputCache;
pub use parser::{NotebookCell, NotebookMetadata, RsParser};

use std::path::Path;

/// Sync a `.rs` notebook to `.ipynb` format.
pub fn sync_to_ipynb(
    rs_path: impl AsRef<Path>,
    ipynb_path: impl AsRef<Path>,
    cache: Option<&OutputCache>,
) -> SyncResult<()> {
    let rs_path = rs_path.as_ref();
    let ipynb_path = ipynb_path.as_ref();

    // Parse the RS file
    let parser = RsParser::new();
    let (metadata, cells) = parser.parse_file(rs_path)?;

    // Generate IPYNB
    let mut generator = IpynbGenerator::new();
    let notebook = generator.generate(&metadata, &cells, cache)?;

    // Write to file
    notebook.write_to_file(ipynb_path)?;

    tracing::info!(
        "Synced {} → {} ({} cells)",
        rs_path.display(),
        ipynb_path.display(),
        cells.len()
    );

    Ok(())
}

/// Get the default `.ipynb` path for a `.rs` notebook.
pub fn default_ipynb_path(rs_path: impl AsRef<Path>) -> std::path::PathBuf {
    let rs_path = rs_path.as_ref();
    rs_path.with_extension("ipynb")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ipynb_path() {
        assert_eq!(
            default_ipynb_path("notebook.rs"),
            std::path::PathBuf::from("notebook.ipynb")
        );
        assert_eq!(
            default_ipynb_path("/path/to/my_notebook.rs"),
            std::path::PathBuf::from("/path/to/my_notebook.ipynb")
        );
    }
}
