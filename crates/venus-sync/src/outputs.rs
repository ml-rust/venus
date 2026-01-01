//! Output cache for storing cell outputs.
//!
//! Caches cell outputs (text, HTML, images) for embedding in `.ipynb` files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{SyncError, SyncResult};
use crate::ipynb::{CellOutput, OutputData};

/// Cache for cell outputs.
pub struct OutputCache {
    /// Cache directory path
    cache_dir: PathBuf,

    /// In-memory cache
    outputs: HashMap<String, CellOutput>,

    /// Execution counter for proper Jupyter numbering
    execution_count: u32,
}

impl OutputCache {
    /// Create a new output cache.
    pub fn new(cache_dir: impl AsRef<Path>) -> SyncResult<Self> {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        fs::create_dir_all(&cache_dir)?;

        let mut cache = Self {
            cache_dir,
            outputs: HashMap::new(),
            execution_count: 0,
        };

        cache.load_from_disk()?;
        Ok(cache)
    }

    /// Get the next execution count.
    fn next_execution_count(&mut self) -> u32 {
        self.execution_count += 1;
        self.execution_count
    }

    /// Create an ExecuteResult output with proper execution count.
    fn make_execute_result(&mut self, data: OutputData) -> CellOutput {
        CellOutput::ExecuteResult {
            execution_count: self.next_execution_count(),
            data,
            metadata: serde_json::json!({}),
        }
    }

    /// Get cached output for a cell.
    pub fn get_output(&self, cell_name: &str) -> Option<CellOutput> {
        self.outputs.get(cell_name).cloned()
    }

    /// Store text output for a cell.
    pub fn store_text(&mut self, cell_name: &str, text: &str) {
        let data = OutputData {
            text_plain: Some(vec![text.to_string()]),
            ..Default::default()
        };
        let output = self.make_execute_result(data);
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Store HTML output for a cell.
    pub fn store_html(&mut self, cell_name: &str, html: &str) {
        let data = OutputData {
            text_html: Some(html.lines().map(String::from).collect()),
            text_plain: Some(vec!["[HTML Output]".to_string()]),
            ..Default::default()
        };
        let output = self.make_execute_result(data);
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Store PNG image output for a cell.
    pub fn store_png(&mut self, cell_name: &str, png_data: &[u8]) {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_data);

        let data = OutputData {
            image_png: Some(encoded),
            text_plain: Some(vec!["[Image]".to_string()]),
            ..Default::default()
        };
        let output = self.make_execute_result(data);
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Store SVG output for a cell.
    pub fn store_svg(&mut self, cell_name: &str, svg: &str) {
        let data = OutputData {
            image_svg: Some(svg.lines().map(String::from).collect()),
            text_plain: Some(vec!["[SVG Image]".to_string()]),
            ..Default::default()
        };
        let output = self.make_execute_result(data);
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Store JSON output for a cell.
    pub fn store_json(&mut self, cell_name: &str, json: serde_json::Value) {
        let text_repr = serde_json::to_string_pretty(&json).unwrap_or_default();
        let data = OutputData {
            application_json: Some(json),
            text_plain: Some(vec![text_repr]),
            ..Default::default()
        };
        let output = self.make_execute_result(data);
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Store error output for a cell.
    pub fn store_error(&mut self, cell_name: &str, error: &str) {
        let output = CellOutput::Error {
            ename: "ExecutionError".to_string(),
            evalue: error.to_string(),
            traceback: error.lines().map(String::from).collect(),
        };
        self.outputs.insert(cell_name.to_string(), output);
    }

    /// Clear all cached outputs.
    pub fn clear(&mut self) {
        self.outputs.clear();
    }

    /// Save cache to disk.
    pub fn save_to_disk(&self) -> SyncResult<()> {
        for (name, output) in &self.outputs {
            let path = self.output_path(name);
            let json = serde_json::to_string_pretty(output)?;
            fs::write(&path, json).map_err(|e| SyncError::WriteError {
                path: path.clone(),
                message: e.to_string(),
            })?;
        }
        Ok(())
    }

    /// Load cache from disk.
    fn load_from_disk(&mut self) -> SyncResult<()> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "json")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(content) = fs::read_to_string(&path)
                && let Ok(output) = serde_json::from_str(&content)
            {
                self.outputs.insert(name.to_string(), output);
            }
        }

        Ok(())
    }

    /// Get the path for a cached output.
    fn output_path(&self, cell_name: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", cell_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve_text() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut cache = OutputCache::new(temp.path()).unwrap();

        cache.store_text("hello", "Hello, world!");

        let output = cache.get_output("hello").unwrap();
        match output {
            CellOutput::ExecuteResult { data, .. } => {
                assert!(data.text_plain.is_some());
                assert!(data.text_plain.unwrap()[0].contains("Hello"));
            }
            _ => panic!("Expected ExecuteResult"),
        }
    }

    #[test]
    fn test_store_and_retrieve_html() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut cache = OutputCache::new(temp.path()).unwrap();

        cache.store_html("table", "<table><tr><td>Data</td></tr></table>");

        let output = cache.get_output("table").unwrap();
        match output {
            CellOutput::ExecuteResult { data, .. } => {
                assert!(data.text_html.is_some());
            }
            _ => panic!("Expected ExecuteResult"),
        }
    }

    #[test]
    fn test_persist_and_reload() {
        let temp = tempfile::TempDir::new().unwrap();

        // Create and populate cache
        {
            let mut cache = OutputCache::new(temp.path()).unwrap();
            cache.store_text("test", "Test output");
            cache.save_to_disk().unwrap();
        }

        // Reload cache
        {
            let cache = OutputCache::new(temp.path()).unwrap();
            assert!(cache.get_output("test").is_some());
        }
    }
}
