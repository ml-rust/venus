//! State manager for Venus notebooks.
//!
//! Handles saving and loading cell outputs with automatic format selection.
//!
//! # Salsa Integration
//!
//! The StateManager can sync its outputs with Salsa's incremental computation
//! system via the [`CellOutputData`] and [`ExecutionStatus`] types. Use:
//!
//! - [`sync_output_to_salsa()`](StateManager::sync_output_to_salsa) to convert
//!   a single output for Salsa tracking
//! - [`sync_all_to_salsa()`](StateManager::sync_all_to_salsa) to export all
//!   outputs as a vector of execution statuses
//! - [`load_from_salsa()`](StateManager::load_from_salsa) to import an output
//!   from Salsa's cached data

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::graph::CellId;
use crate::salsa_db::{CellOutputData, ExecutionStatus};

use super::output::BoxedOutput;
use super::schema::{SchemaChange, TypeFingerprint};

/// Manages cell state persistence and invalidation.
pub struct StateManager {
    /// Base directory for state storage
    state_dir: PathBuf,

    /// In-memory cache of cell outputs
    outputs: HashMap<CellId, Arc<BoxedOutput>>,

    /// Type fingerprints for schema validation
    fingerprints: HashMap<CellId, TypeFingerprint>,

    /// Dirty cells that need to be persisted.
    /// Uses HashSet to avoid duplicate writes when save() is called multiple times.
    dirty: HashSet<CellId>,
}

impl StateManager {
    /// Create a new state manager with the given state directory.
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self> {
        let state_dir = state_dir.as_ref().to_path_buf();
        fs::create_dir_all(&state_dir)?;

        Ok(Self {
            state_dir,
            outputs: HashMap::new(),
            fingerprints: HashMap::new(),
            dirty: HashSet::new(),
        })
    }

    /// Save a cell output.
    pub fn save<T: super::output::CellOutput>(&mut self, cell_id: CellId, value: &T) -> Result<()> {
        let boxed = BoxedOutput::new(value)?;
        self.outputs.insert(cell_id, Arc::new(boxed));
        self.dirty.insert(cell_id);
        Ok(())
    }

    /// Load a cell output.
    pub fn load<T: super::output::CellOutput + bincode::Decode<()>>(
        &self,
        cell_id: CellId,
    ) -> Result<T> {
        // Try in-memory cache first
        if let Some(boxed) = self.outputs.get(&cell_id) {
            return boxed.deserialize();
        }

        // Try loading from disk
        let path = self.output_path(cell_id);
        if path.exists() {
            let bytes = fs::read(&path)?;
            let (boxed, _): (BoxedOutput, _) =
                bincode::decode_from_slice(&bytes, bincode::config::standard())
                    .map_err(|e| Error::Deserialization(e.to_string()))?;
            return boxed.deserialize();
        }

        Err(Error::CellNotFound(format!(
            "No output for cell {:?}",
            cell_id
        )))
    }

    /// Get a reference to a cached output without deserializing.
    pub fn get_output(&self, cell_id: CellId) -> Option<Arc<BoxedOutput>> {
        self.outputs.get(&cell_id).cloned()
    }

    /// Store a pre-serialized output directly.
    ///
    /// Used by the execution engine to store outputs from FFI calls.
    pub fn store_output(&mut self, cell_id: CellId, output: BoxedOutput) {
        self.outputs.insert(cell_id, Arc::new(output));
        self.dirty.insert(cell_id);
    }

    /// Check if a cell has a cached output.
    pub fn has_output(&self, cell_id: CellId) -> bool {
        self.outputs.contains_key(&cell_id) || self.output_path(cell_id).exists()
    }

    /// Invalidate a cell's output (e.g., when its source changes).
    pub fn invalidate(&mut self, cell_id: CellId) {
        self.outputs.remove(&cell_id);
        self.fingerprints.remove(&cell_id);

        // Remove from disk
        let path = self.output_path(cell_id);
        let _ = fs::remove_file(path);
    }

    /// Invalidate multiple cells.
    pub fn invalidate_many(&mut self, cell_ids: &[CellId]) {
        for &cell_id in cell_ids {
            self.invalidate(cell_id);
        }
    }

    /// Called when a cell is modified - invalidates it and all dependents.
    ///
    /// Returns the list of invalidated cell IDs.
    pub fn on_cell_modified(&mut self, cell_id: CellId, dependents: &[CellId]) -> Vec<CellId> {
        let mut invalidated = vec![cell_id];
        invalidated.extend_from_slice(dependents);

        for &id in &invalidated {
            self.invalidate(id);
        }

        invalidated
    }

    /// Update the type fingerprint for a cell and check for schema changes.
    pub fn update_fingerprint(
        &mut self,
        cell_id: CellId,
        new_fingerprint: TypeFingerprint,
    ) -> SchemaChange {
        if let Some(old) = self.fingerprints.get(&cell_id) {
            let change = old.compare(&new_fingerprint);

            if change.is_breaking() {
                // Invalidate cached output on breaking change
                self.invalidate(cell_id);
                tracing::warn!(
                    "Schema change for cell {:?}: {}",
                    cell_id,
                    change.description()
                );
            }

            self.fingerprints.insert(cell_id, new_fingerprint);
            change
        } else {
            self.fingerprints.insert(cell_id, new_fingerprint);
            SchemaChange::None
        }
    }

    /// Persist all dirty outputs to disk.
    pub fn flush(&mut self) -> Result<()> {
        let dirty_cells: Vec<_> = self.dirty.drain().collect();
        for cell_id in dirty_cells {
            if let Some(boxed) = self.outputs.get(&cell_id) {
                let path = self.output_path(cell_id);

                // Ensure parent directory exists
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }

                let bytes = bincode::encode_to_vec(boxed.as_ref(), bincode::config::standard())
                    .map_err(|e| Error::Serialization(e.to_string()))?;

                fs::write(&path, bytes)?;
            }
        }
        Ok(())
    }

    /// Load all cached outputs from disk.
    pub fn restore(&mut self) -> Result<usize> {
        let outputs_dir = self.state_dir.join("outputs");
        if !outputs_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in fs::read_dir(&outputs_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "bin")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && let Ok(id) = stem.parse::<usize>()
            {
                let cell_id = CellId::new(id);
                let bytes = fs::read(&path)?;

                match bincode::decode_from_slice::<BoxedOutput, _>(
                    &bytes,
                    bincode::config::standard(),
                ) {
                    Ok((boxed, _)) => {
                        self.outputs.insert(cell_id, Arc::new(boxed));
                        count += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to restore output for cell {}: {}", id, e);
                    }
                }
            }
        }

        tracing::info!("Restored {} cached outputs", count);
        Ok(count)
    }

    /// Get the path for a cell's output file.
    fn output_path(&self, cell_id: CellId) -> PathBuf {
        self.state_dir
            .join("outputs")
            .join(format!("{}.bin", cell_id.as_usize()))
    }

    // =========================================================================
    // Salsa Integration
    // =========================================================================

    /// Convert a single cell output to Salsa-compatible format.
    ///
    /// Returns `None` if the cell has no cached output.
    ///
    /// # Arguments
    ///
    /// * `cell_id` - The cell to export
    /// * `inputs_hash` - Hash of the cell's input values (for staleness detection)
    /// * `execution_time_ms` - How long the cell took to execute
    pub fn sync_output_to_salsa(
        &self,
        cell_id: CellId,
        inputs_hash: u64,
        execution_time_ms: u64,
    ) -> Option<CellOutputData> {
        self.outputs.get(&cell_id).map(|boxed| {
            CellOutputData::from_boxed(cell_id.as_usize(), boxed, inputs_hash, execution_time_ms)
        })
    }

    /// Export all outputs to a vector of execution statuses for Salsa.
    ///
    /// Creates a vector sized to `cell_count` where each index corresponds
    /// to a cell ID. Cells without outputs are marked as `Pending`.
    ///
    /// # Arguments
    ///
    /// * `cell_count` - Total number of cells in the notebook
    /// * `get_inputs_hash` - Closure to get the inputs hash for each cell
    /// * `get_execution_time` - Closure to get execution time for each cell (0 if unknown)
    pub fn sync_all_to_salsa<F, G>(
        &self,
        cell_count: usize,
        get_inputs_hash: F,
        get_execution_time: G,
    ) -> Vec<ExecutionStatus>
    where
        F: Fn(CellId) -> u64,
        G: Fn(CellId) -> u64,
    {
        (0..cell_count)
            .map(|idx| {
                let cell_id = CellId::new(idx);
                if let Some(boxed) = self.outputs.get(&cell_id) {
                    let output_data = CellOutputData::from_boxed(
                        idx,
                        boxed,
                        get_inputs_hash(cell_id),
                        get_execution_time(cell_id),
                    );
                    ExecutionStatus::Success(output_data)
                } else {
                    ExecutionStatus::Pending
                }
            })
            .collect()
    }

    /// Import an output from Salsa's cached data.
    ///
    /// Converts a `CellOutputData` back to a `BoxedOutput` and stores it.
    pub fn load_from_salsa(&mut self, output_data: &CellOutputData) {
        let cell_id = CellId::new(output_data.cell_id);
        let boxed = output_data.to_boxed();
        self.outputs.insert(cell_id, Arc::new(boxed));
        // Don't mark as dirty - this came from Salsa, not from execution
    }

    /// Import all successful outputs from Salsa's execution statuses.
    ///
    /// Returns the number of outputs imported.
    pub fn load_all_from_salsa(&mut self, statuses: &[ExecutionStatus]) -> usize {
        let mut count = 0;
        for status in statuses {
            if let ExecutionStatus::Success(output_data) = status {
                self.load_from_salsa(output_data);
                count += 1;
            }
        }
        count
    }

    /// Check if a Salsa output is still valid for the current inputs.
    ///
    /// Note: The StateManager doesn't store input hashes, so this method only
    /// checks if an output exists. For actual validation, use the `is_valid_for()`
    /// method on `CellOutputData` with the Salsa-cached output.
    ///
    /// The `_current_inputs_hash` parameter is reserved for future use when
    /// input hash tracking is added to the StateManager.
    pub fn is_salsa_output_valid(&self, cell_id: CellId, _current_inputs_hash: u64) -> bool {
        self.has_output(cell_id)
    }

    /// Clear all state (for testing or reset).
    pub fn clear(&mut self) -> Result<()> {
        self.outputs.clear();
        self.fingerprints.clear();
        self.dirty.clear();

        let outputs_dir = self.state_dir.join("outputs");
        if outputs_dir.exists() {
            fs::remove_dir_all(&outputs_dir)?;
        }

        Ok(())
    }

    /// Get statistics about the state manager.
    pub fn stats(&self) -> StateStats {
        StateStats {
            cached_outputs: self.outputs.len(),
            dirty_outputs: self.dirty.len(),
            fingerprints: self.fingerprints.len(),
        }
    }
}

/// Statistics about the state manager.
#[derive(Debug, Clone)]
pub struct StateStats {
    /// Number of outputs in memory cache
    pub cached_outputs: usize,

    /// Number of outputs pending persistence
    pub dirty_outputs: usize,

    /// Number of type fingerprints tracked
    pub fingerprints: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{Decode, Encode};
    use tempfile::TempDir;

    #[derive(Debug, Clone, PartialEq, Encode, Decode)]
    struct TestOutput {
        value: i32,
    }

    fn setup() -> (StateManager, TempDir) {
        let temp = TempDir::new().unwrap();
        let manager = StateManager::new(temp.path()).unwrap();
        (manager, temp)
    }

    #[test]
    fn test_save_and_load() {
        let (mut manager, _temp) = setup();
        let cell_id = CellId::new(0);

        let output = TestOutput { value: 42 };
        manager.save(cell_id, &output).unwrap();

        let loaded: TestOutput = manager.load(cell_id).unwrap();
        assert_eq!(output, loaded);
    }

    #[test]
    fn test_invalidate() {
        let (mut manager, _temp) = setup();
        let cell_id = CellId::new(0);

        let output = TestOutput { value: 42 };
        manager.save(cell_id, &output).unwrap();

        assert!(manager.has_output(cell_id));
        manager.invalidate(cell_id);
        assert!(!manager.has_output(cell_id));
    }

    #[test]
    fn test_persist_and_restore() {
        let temp = TempDir::new().unwrap();
        let cell_id = CellId::new(0);

        {
            let mut manager = StateManager::new(temp.path()).unwrap();
            let output = TestOutput { value: 42 };
            manager.save(cell_id, &output).unwrap();
            manager.flush().unwrap();
        }

        {
            let mut manager = StateManager::new(temp.path()).unwrap();
            manager.restore().unwrap();
            let loaded: TestOutput = manager.load(cell_id).unwrap();
            assert_eq!(loaded.value, 42);
        }
    }

    #[test]
    fn test_on_cell_modified() {
        let (mut manager, _temp) = setup();

        let cell0 = CellId::new(0);
        let cell1 = CellId::new(1);
        let cell2 = CellId::new(2);

        // Save outputs for all cells
        manager.save(cell0, &TestOutput { value: 0 }).unwrap();
        manager.save(cell1, &TestOutput { value: 1 }).unwrap();
        manager.save(cell2, &TestOutput { value: 2 }).unwrap();

        // Modify cell0, which has dependents cell1 and cell2
        let invalidated = manager.on_cell_modified(cell0, &[cell1, cell2]);

        assert_eq!(invalidated.len(), 3);
        assert!(!manager.has_output(cell0));
        assert!(!manager.has_output(cell1));
        assert!(!manager.has_output(cell2));
    }

    #[test]
    fn test_schema_change_detection() {
        let (mut manager, _temp) = setup();
        let cell_id = CellId::new(0);

        // Save initial output
        manager.save(cell_id, &TestOutput { value: 42 }).unwrap();

        // First fingerprint
        let fp1 =
            TypeFingerprint::new("TestOutput", vec![("value".to_string(), "i32".to_string())]);
        let change = manager.update_fingerprint(cell_id, fp1);
        assert!(!change.is_breaking());

        // Same fingerprint
        let fp2 =
            TypeFingerprint::new("TestOutput", vec![("value".to_string(), "i32".to_string())]);
        let change = manager.update_fingerprint(cell_id, fp2);
        assert!(!change.is_breaking());
        assert!(manager.has_output(cell_id)); // Still cached

        // Breaking change
        let fp3 = TypeFingerprint::new(
            "TestOutput",
            vec![("value".to_string(), "i64".to_string())], // Type changed!
        );
        let change = manager.update_fingerprint(cell_id, fp3);
        assert!(change.is_breaking());
        assert!(!manager.has_output(cell_id)); // Invalidated
    }

    #[test]
    fn test_sync_output_to_salsa() {
        let (mut manager, _temp) = setup();
        let cell_id = CellId::new(0);

        // No output yet
        assert!(manager.sync_output_to_salsa(cell_id, 12345, 100).is_none());

        // Save an output
        manager.save(cell_id, &TestOutput { value: 42 }).unwrap();

        // Now we can sync to Salsa
        let output_data = manager.sync_output_to_salsa(cell_id, 12345, 100).unwrap();
        assert_eq!(output_data.cell_id, 0);
        assert_eq!(output_data.inputs_hash, 12345);
        assert_eq!(output_data.execution_time_ms, 100);
        assert!(!output_data.bytes.is_empty());
    }

    #[test]
    fn test_sync_all_to_salsa() {
        let (mut manager, _temp) = setup();

        // Save outputs for cells 0 and 2, skip cell 1
        manager.save(CellId::new(0), &TestOutput { value: 0 }).unwrap();
        manager.save(CellId::new(2), &TestOutput { value: 2 }).unwrap();

        let statuses = manager.sync_all_to_salsa(
            3,
            |cell_id| cell_id.as_usize() as u64 * 100, // inputs_hash
            |cell_id| cell_id.as_usize() as u64 * 10,  // execution_time
        );

        assert_eq!(statuses.len(), 3);
        assert!(matches!(statuses[0], ExecutionStatus::Success(_)));
        assert!(matches!(statuses[1], ExecutionStatus::Pending));
        assert!(matches!(statuses[2], ExecutionStatus::Success(_)));

        // Check the output data for cell 0
        if let ExecutionStatus::Success(data) = &statuses[0] {
            assert_eq!(data.inputs_hash, 0);
            assert_eq!(data.execution_time_ms, 0);
        }

        // Check the output data for cell 2
        if let ExecutionStatus::Success(data) = &statuses[2] {
            assert_eq!(data.inputs_hash, 200);
            assert_eq!(data.execution_time_ms, 20);
        }
    }

    #[test]
    fn test_load_from_salsa() {
        let (mut manager, _temp) = setup();
        let cell_id = CellId::new(0);

        // Create a CellOutputData directly
        let output = TestOutput { value: 99 };
        let boxed = BoxedOutput::new(&output).unwrap();
        let output_data = CellOutputData::from_boxed(0, &boxed, 12345, 50);

        // Load from Salsa
        manager.load_from_salsa(&output_data);

        // Verify we can retrieve it
        assert!(manager.has_output(cell_id));
        let loaded: TestOutput = manager.load(cell_id).unwrap();
        assert_eq!(loaded.value, 99);

        // Should NOT be marked dirty (came from Salsa, not execution)
        assert!(!manager.dirty.contains(&cell_id));
    }

    #[test]
    fn test_load_all_from_salsa() {
        let (mut manager, _temp) = setup();

        // Create some execution statuses
        let output0 = TestOutput { value: 100 };
        let boxed0 = BoxedOutput::new(&output0).unwrap();
        let data0 = CellOutputData::from_boxed(0, &boxed0, 0, 0);

        let output2 = TestOutput { value: 200 };
        let boxed2 = BoxedOutput::new(&output2).unwrap();
        let data2 = CellOutputData::from_boxed(2, &boxed2, 0, 0);

        let statuses = vec![
            ExecutionStatus::Success(data0),
            ExecutionStatus::Pending,
            ExecutionStatus::Success(data2),
            ExecutionStatus::Failed("error".to_string()),
        ];

        // Load from Salsa
        let count = manager.load_all_from_salsa(&statuses);
        assert_eq!(count, 2); // Only successful ones

        // Verify outputs
        assert!(manager.has_output(CellId::new(0)));
        assert!(!manager.has_output(CellId::new(1))); // Was pending
        assert!(manager.has_output(CellId::new(2)));
        assert!(!manager.has_output(CellId::new(3))); // Was failed

        let loaded0: TestOutput = manager.load(CellId::new(0)).unwrap();
        assert_eq!(loaded0.value, 100);

        let loaded2: TestOutput = manager.load(CellId::new(2)).unwrap();
        assert_eq!(loaded2.value, 200);
    }
}
