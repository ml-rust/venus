//! Execution context and callbacks for Venus cells.
//!
//! Provides resource management, progress reporting, and cooperative cancellation
//! during cell execution.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::Error;
use crate::graph::CellId;

/// Handle for cooperative cancellation of cell execution.
///
/// `AbortHandle` provides a thread-safe mechanism for signaling that execution
/// should be cancelled. It can be cloned and shared across threads, and any
/// clone can trigger the abort which will be visible to all other clones.
///
/// # Example
///
/// ```
/// use venus_core::execute::AbortHandle;
///
/// let handle = AbortHandle::new();
/// let handle_clone = handle.clone();
///
/// // Check abort status
/// assert!(!handle.is_aborted());
///
/// // Trigger abort from any clone
/// handle_clone.abort();
///
/// // All clones see the abort
/// assert!(handle.is_aborted());
/// ```
#[derive(Clone, Default)]
pub struct AbortHandle {
    /// Shared abort flag.
    aborted: Arc<AtomicBool>,
}

impl AbortHandle {
    /// Create a new abort handle.
    pub fn new() -> Self {
        Self {
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if abort has been requested.
    ///
    /// Cells should call this periodically during long-running operations
    /// and exit early if it returns `true`.
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// Request abort of execution.
    ///
    /// This is a cooperative mechanism - cells must check `is_aborted()`
    /// and honor the request by returning early.
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    /// Reset the abort flag.
    ///
    /// Called before starting a new execution to clear any previous abort.
    pub fn reset(&self) {
        self.aborted.store(false, Ordering::Relaxed);
    }
}

/// Callback trait for execution progress reporting.
pub trait ExecutionCallback: Send + Sync {
    /// Called when a cell starts executing.
    fn on_cell_started(&self, cell_id: CellId, name: &str);

    /// Called when a cell completes successfully.
    fn on_cell_completed(&self, cell_id: CellId, name: &str);

    /// Called when a cell execution fails.
    fn on_cell_error(&self, cell_id: CellId, name: &str, error: &Error);

    /// Called when a parallel level starts.
    fn on_level_started(&self, _level: usize, _cell_count: usize) {}

    /// Called when a parallel level completes.
    fn on_level_completed(&self, _level: usize) {}
}

// Note: LoggingCallback was removed as unused dead code.
// Users can implement ExecutionCallback trait directly for custom logging.

/// Execution context for a running cell.
///
/// Provides resource management and cleanup hooks for cells that
/// need to manage background tasks or external resources.
pub struct CellContext {
    /// Cell identifier
    cell_id: CellId,
    /// Cell name for logging
    name: String,
    /// Registered cleanup handlers
    cleanup_handlers: Vec<Box<dyn FnOnce() + Send>>,
    /// Whether the cell has been aborted
    aborted: bool,
}

impl CellContext {
    /// Create a new cell context.
    pub fn new(cell_id: CellId, name: String) -> Self {
        Self {
            cell_id,
            name,
            cleanup_handlers: Vec::new(),
            aborted: false,
        }
    }

    /// Get the cell ID.
    pub fn cell_id(&self) -> CellId {
        self.cell_id
    }

    /// Get the cell name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if execution has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    /// Register a cleanup handler to be called when the cell is unloaded.
    ///
    /// Cleanup handlers are called in reverse order of registration.
    pub fn on_cleanup(&mut self, handler: impl FnOnce() + Send + 'static) {
        self.cleanup_handlers.push(Box::new(handler));
    }

    /// Abort cell execution.
    ///
    /// This sets the aborted flag and runs all cleanup handlers.
    pub fn abort(&mut self) {
        if !self.aborted {
            self.aborted = true;
            self.run_cleanup();
        }
    }

    /// Run all cleanup handlers.
    fn run_cleanup(&mut self) {
        // Run handlers in reverse order
        while let Some(handler) = self.cleanup_handlers.pop() {
            // Catch panics to ensure all handlers run
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(handler));
            if let Err(e) = result {
                tracing::error!(
                    "Cleanup handler for cell {:?} panicked: {:?}",
                    self.cell_id,
                    e
                );
            }
        }
    }
}

impl Drop for CellContext {
    fn drop(&mut self) {
        self.run_cleanup();
    }
}

// Note: SharedContext types were removed as unused dead code.
// ParallelExecutor uses LinearExecutor internally with a Mutex, which is sufficient for current needs.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_cell_context_creation() {
        let ctx = CellContext::new(CellId::new(0), "test".to_string());
        assert_eq!(ctx.cell_id().as_usize(), 0);
        assert_eq!(ctx.name(), "test");
        assert!(!ctx.is_aborted());
    }

    #[test]
    fn test_cleanup_handlers() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        {
            let mut ctx = CellContext::new(CellId::new(0), "test".to_string());
            ctx.on_cleanup(move || {
                called_clone.store(true, Ordering::SeqCst);
            });
        }

        // Handler should be called when context is dropped
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_cleanup_order() {
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let order1 = order.clone();
        let order2 = order.clone();

        {
            let mut ctx = CellContext::new(CellId::new(0), "test".to_string());
            ctx.on_cleanup(move || {
                order1.lock().unwrap().push(1);
            });
            ctx.on_cleanup(move || {
                order2.lock().unwrap().push(2);
            });
        }

        // Handlers should be called in reverse order (LIFO)
        assert_eq!(*order.lock().unwrap(), vec![2, 1]);
    }

    #[test]
    fn test_abort() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let mut ctx = CellContext::new(CellId::new(0), "test".to_string());
        ctx.on_cleanup(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(!ctx.is_aborted());
        ctx.abort();
        assert!(ctx.is_aborted());
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_abort_handle_creation() {
        let handle = AbortHandle::new();
        assert!(!handle.is_aborted());
    }

    #[test]
    fn test_abort_handle_abort() {
        let handle = AbortHandle::new();
        assert!(!handle.is_aborted());

        handle.abort();
        assert!(handle.is_aborted());
    }

    #[test]
    fn test_abort_handle_clone_shares_state() {
        let handle = AbortHandle::new();
        let clone = handle.clone();

        assert!(!handle.is_aborted());
        assert!(!clone.is_aborted());

        clone.abort();

        assert!(handle.is_aborted());
        assert!(clone.is_aborted());
    }

    #[test]
    fn test_abort_handle_reset() {
        let handle = AbortHandle::new();
        handle.abort();
        assert!(handle.is_aborted());

        handle.reset();
        assert!(!handle.is_aborted());
    }

    #[test]
    fn test_abort_handle_default() {
        let handle = AbortHandle::default();
        assert!(!handle.is_aborted());
    }
}
