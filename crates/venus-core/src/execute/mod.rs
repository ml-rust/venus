//! Execution engine for Venus notebooks.
//!
//! Provides linear, parallel, and process-isolated cell execution with hot-reload support.
//!
//! # Executors
//!
//! - **`LinearExecutor`** - In-process sequential execution. Fast but no isolation.
//! - **`ParallelExecutor`** - In-process parallel execution using Rayon.
//! - **`ProcessExecutor`** - Process-isolated execution. Cells run in worker processes
//!   that can be killed for true interruption. Provides crash isolation, memory isolation,
//!   and immediate cancellation.
//!
//! # Architecture
//!
//! ## In-Process Execution (LinearExecutor, ParallelExecutor)
//!
//! ```text
//! CompiledCell
//!     │
//!     └── LoadedCell (dylib loaded via libloading)
//!             │
//!             └── LinearExecutor / ParallelExecutor
//!                     │
//!                     └── FFI call → cell entry point
//!                             │
//!                             └── Output stored in StateManager
//! ```
//!
//! ## Process-Isolated Execution (ProcessExecutor)
//!
//! ```text
//! ProcessExecutor (parent)
//!     │
//!     └── WorkerPool
//!             │
//!             └── WorkerHandle (manages child process)
//!                     │
//!                     ├── IPC: LoadCell command
//!                     │       └── venus-worker loads dylib
//!                     │
//!                     ├── IPC: Execute command
//!                     │       └── venus-worker calls FFI
//!                     │       └── Returns serialized output
//!                     │
//!                     └── SIGKILL for immediate interruption
//! ```
//!
//! # Module Structure
//!
//! - `context` - Execution callbacks and cell context
//! - `executor` - LinearExecutor for sequential execution
//! - `ffi` - FFI types and dispatch macros
//! - `loaded_cell` - LoadedCell wrapper for dylibs
//! - `parallel` - ParallelExecutor for concurrent execution
//! - `process` - ProcessExecutor for isolated execution
//! - `reload` - Hot-reload support
//! - `windows_dll` - Windows DLL hot-reload handler

mod context;
mod executor;
mod ffi;
mod loaded_cell;
mod parallel;
mod process;
mod reload;
mod windows_dll;

pub use context::{AbortHandle, CellContext, ExecutionCallback};
pub use executor::LinearExecutor;
pub use ffi::ExecutionResult;
pub use loaded_cell::LoadedCell;
pub use parallel::ParallelExecutor;
pub use process::{ExecutorKillHandle, ProcessExecutor};
pub use reload::HotReloader;
pub use windows_dll::WindowsDllHandler;
