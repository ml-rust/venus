//! Worker process management for Venus cell execution.
//!
//! Provides `WorkerHandle` for spawning and communicating with isolated
//! worker processes, and `WorkerPool` for efficient worker reuse.

use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};

use super::protocol::{WorkerCommand, WorkerResponse, read_message, write_message};

/// Handle to a worker process.
///
/// Provides methods to send commands, receive responses, and kill the process.
pub struct WorkerHandle {
    /// The child process.
    child: Child,
    /// Buffered stdin writer.
    stdin: BufWriter<std::process::ChildStdin>,
    /// Buffered stdout reader.
    stdout: BufReader<std::process::ChildStdout>,
    /// Whether the worker has been killed.
    killed: bool,
}

impl WorkerHandle {
    /// Spawn a new worker process.
    ///
    /// Looks for the `venus-worker` binary in the following order:
    /// 1. `VENUS_WORKER_PATH` environment variable
    /// 2. Same directory as the current executable
    /// 3. System PATH
    pub fn spawn() -> Result<Self> {
        let worker_path = Self::find_worker_binary()?;

        let mut child = Command::new(&worker_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Let worker stderr pass through for debugging
            .spawn()
            .map_err(|e| {
                Error::Ipc(format!(
                    "Failed to spawn worker process '{}': {}",
                    worker_path.display(),
                    e
                ))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            Error::Ipc("Failed to get worker stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            Error::Ipc("Failed to get worker stdout".to_string())
        })?;

        let mut handle = Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            killed: false,
        };

        // Verify worker is alive with a ping
        handle.send_command(&WorkerCommand::Ping)?;
        match handle.recv_response()? {
            WorkerResponse::Pong => Ok(handle),
            other => Err(Error::Ipc(format!(
                "Unexpected response from worker: {:?}",
                other
            ))),
        }
    }

    /// Find the venus-worker binary path.
    fn find_worker_binary() -> Result<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var("VENUS_WORKER_PATH") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
        }

        // 2. Look next to current executable
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent() {
                let worker_name = if cfg!(windows) {
                    "venus-worker.exe"
                } else {
                    "venus-worker"
                };
                let worker_path = exe_dir.join(worker_name);
                if worker_path.exists() {
                    return Ok(worker_path);
                }
            }

        // 3. Try system PATH via which
        let worker_name = if cfg!(windows) {
            "venus-worker.exe"
        } else {
            "venus-worker"
        };
        if let Ok(path) = which::which(worker_name) {
            return Ok(path);
        }

        // 4. For development: try target/debug or target/release
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            for profile in &["debug", "release"] {
                let worker_name = if cfg!(windows) {
                    "venus-worker.exe"
                } else {
                    "venus-worker"
                };
                let path = PathBuf::from(&manifest_dir)
                    .join("..")
                    .join("..")
                    .join("target")
                    .join(profile)
                    .join(worker_name);
                if path.exists() {
                    return Ok(path.canonicalize().unwrap_or(path));
                }
            }
        }

        Err(Error::Ipc(
            "Could not find venus-worker binary. Set VENUS_WORKER_PATH or ensure it's in PATH."
                .to_string(),
        ))
    }

    /// Send a command to the worker.
    pub fn send_command(&mut self, cmd: &WorkerCommand) -> Result<()> {
        if self.killed {
            return Err(Error::Ipc("Worker has been killed".to_string()));
        }
        write_message(&mut self.stdin, cmd)
    }

    /// Receive a response from the worker.
    pub fn recv_response(&mut self) -> Result<WorkerResponse> {
        if self.killed {
            return Err(Error::Ipc("Worker has been killed".to_string()));
        }
        read_message(&mut self.stdout)
    }

    /// Load a cell in the worker.
    pub fn load_cell(
        &mut self,
        dylib_path: PathBuf,
        dep_count: usize,
        entry_symbol: String,
        name: String,
    ) -> Result<()> {
        self.send_command(&WorkerCommand::LoadCell {
            dylib_path: dylib_path.to_string_lossy().to_string(),
            dep_count,
            entry_symbol,
            name,
        })?;

        match self.recv_response()? {
            WorkerResponse::Loaded => Ok(()),
            WorkerResponse::Error { message } => {
                Err(Error::Execution(format!("Failed to load cell: {}", message)))
            }
            other => Err(Error::Ipc(format!(
                "Unexpected response when loading cell: {:?}",
                other
            ))),
        }
    }

    /// Execute the loaded cell with given inputs.
    ///
    /// Returns the raw output bytes on success.
    pub fn execute(&mut self, inputs: Vec<Vec<u8>>) -> Result<Vec<u8>> {
        self.execute_with_widgets(inputs, Vec::new()).map(|(bytes, _)| bytes)
    }

    /// Execute the loaded cell with given inputs and widget values.
    ///
    /// Returns the raw output bytes and widget definitions JSON on success.
    pub fn execute_with_widgets(
        &mut self,
        inputs: Vec<Vec<u8>>,
        widget_values_json: Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        self.send_command(&WorkerCommand::Execute { inputs, widget_values_json })?;

        match self.recv_response()? {
            WorkerResponse::Output { bytes, widgets_json } => Ok((bytes, widgets_json)),
            WorkerResponse::Error { message } => {
                Err(Error::Execution(message))
            }
            WorkerResponse::Panic { message } => {
                Err(Error::Execution(format!(
                    "Cell panicked: {}. Check for unwrap() on None/Err, out-of-bounds access, or other panic sources.",
                    message
                )))
            }
            other => Err(Error::Ipc(format!(
                "Unexpected response when executing: {:?}",
                other
            ))),
        }
    }

    /// Kill the worker process immediately.
    ///
    /// This is the key feature for interruption - we can terminate
    /// the worker mid-computation without any cooperation from the cell.
    pub fn kill(&mut self) -> Result<()> {
        if self.killed {
            return Ok(());
        }

        self.killed = true;

        // Try graceful shutdown first (with short timeout)
        // This allows any cleanup to happen
        let _ = self.send_command(&WorkerCommand::Shutdown);

        // Give it a moment to shutdown gracefully
        std::thread::sleep(Duration::from_millis(10));

        // Force kill if still running
        if let Err(e) = self.child.kill() {
            // ESRCH means process already exited, which is fine
            if !e.to_string().contains("No such process") {
                tracing::warn!("Failed to kill worker: {}", e);
            }
        }

        // Wait to reap zombie
        let _ = self.child.wait();

        Ok(())
    }

    /// Check if the worker process is still running.
    pub fn is_alive(&mut self) -> bool {
        if self.killed {
            return false;
        }
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Get the process ID of the worker.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Graceful shutdown - ask worker to exit cleanly.
    pub fn shutdown(mut self) -> Result<()> {
        if self.killed {
            return Ok(());
        }

        let _ = self.send_command(&WorkerCommand::Shutdown);

        // Wait for acknowledgement with timeout
        // Note: We can't easily do timeout on blocking read,
        // so we just wait for the process to exit
        match self.child.wait() {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::Ipc(format!(
                        "Worker exited with status: {}",
                        status
                    )))
                }
            }
            Err(e) => Err(Error::Ipc(format!("Failed to wait for worker: {}", e))),
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Ensure worker is killed when handle is dropped
        let _ = self.kill();
    }
}

/// Pool of reusable worker processes.
///
/// Maintains a set of warm workers to avoid spawn overhead.
/// Workers are recycled after each cell execution.
pub struct WorkerPool {
    /// Available workers ready for use.
    available: Vec<WorkerHandle>,
    /// Maximum pool size.
    max_size: usize,
}

impl WorkerPool {
    /// Create a new worker pool.
    pub fn new(max_size: usize) -> Self {
        Self {
            available: Vec::with_capacity(max_size),
            max_size,
        }
    }

    /// Create a pool and pre-warm with N workers.
    pub fn with_warm_workers(max_size: usize, warm_count: usize) -> Result<Self> {
        let mut pool = Self::new(max_size);
        for _ in 0..warm_count.min(max_size) {
            let worker = WorkerHandle::spawn()?;
            pool.available.push(worker);
        }
        Ok(pool)
    }

    /// Get a worker from the pool, spawning if necessary.
    pub fn get(&mut self) -> Result<WorkerHandle> {
        // Try to reuse an existing worker
        while let Some(mut worker) = self.available.pop() {
            if worker.is_alive() {
                return Ok(worker);
            }
            // Worker died, try next one
        }

        // No available workers, spawn a new one
        WorkerHandle::spawn()
    }

    /// Return a worker to the pool for reuse.
    ///
    /// If the pool is full, the worker is dropped (killed).
    pub fn put(&mut self, mut worker: WorkerHandle) {
        if !worker.is_alive() {
            return;
        }

        if self.available.len() < self.max_size {
            self.available.push(worker);
        }
        // Otherwise worker is dropped and killed
    }

    /// Kill all workers in the pool.
    pub fn shutdown(&mut self) {
        for mut worker in self.available.drain(..) {
            let _ = worker.kill();
        }
    }

    /// Get the number of available workers.
    pub fn available_count(&self) -> usize {
        self.available.len()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Thread-safe handle for killing a worker from another thread.
///
/// Used for interrupt handling in async contexts.
#[derive(Clone)]
pub struct WorkerKillHandle {
    /// Process ID of the worker.
    pid: u32,
    /// Whether the kill has been requested.
    killed: Arc<std::sync::atomic::AtomicBool>,
}

impl WorkerKillHandle {
    /// Create a kill handle for a worker.
    pub fn new(worker: &WorkerHandle) -> Self {
        Self {
            pid: worker.pid(),
            killed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Kill the worker process.
    ///
    /// This can be called from any thread and will immediately
    /// terminate the worker process.
    pub fn kill(&self) {
        if self.killed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return; // Already killed
        }

        #[cfg(unix)]
        {
            // SIGKILL for immediate termination
            unsafe {
                libc::kill(self.pid as i32, libc::SIGKILL);
            }
        }

        #[cfg(windows)]
        {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

            unsafe {
                if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, self.pid) {
                    let _ = TerminateProcess(handle, 1);
                    let _ = CloseHandle(handle);
                }
            }
        }
    }

    /// Check if kill has been requested.
    pub fn is_killed(&self) -> bool {
        self.killed.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require the venus-worker binary to be built.
    // Run `cargo build -p venus-worker` first.

    #[test]
    #[ignore = "Requires venus-worker binary"]
    fn test_worker_spawn_and_ping() {
        let worker = WorkerHandle::spawn().unwrap();
        assert!(worker.pid() > 0);
    }

    #[test]
    #[ignore = "Requires venus-worker binary"]
    fn test_worker_pool() {
        let mut pool = WorkerPool::new(4);
        let worker1 = pool.get().unwrap();
        let pid1 = worker1.pid();
        pool.put(worker1);

        let worker2 = pool.get().unwrap();
        assert_eq!(worker2.pid(), pid1); // Same worker reused
    }
}
