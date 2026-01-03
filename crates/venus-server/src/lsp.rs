//! LSP (Language Server Protocol) proxy for rust-analyzer.
//!
//! Provides a WebSocket endpoint that proxies LSP messages between
//! the Monaco editor frontend and a rust-analyzer instance.
//!
//! rust-analyzer is automatically downloaded and cached if not available.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::rust_analyzer;

lazy_static::lazy_static! {
    /// Global registry of all running rust-analyzer processes.
    ///
    /// Multi-layered cleanup strategy:
    /// 1. **Linux**: `prctl(PR_SET_PDEATHSIG)` kills child if parent dies (crash, kill -9, etc.)
    /// 2. **Windows**: Job object kills child when job handle is closed (parent dies)
    /// 3. **Graceful shutdown**: Ctrl+C handler calls `kill_all_processes()`
    /// 4. **WebSocket close**: Each LSP session kills its own rust-analyzer on disconnect
    /// 5. **Fallback**: This registry tracks all PIDs for manual cleanup
    static ref ANALYZER_PROCESSES: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
}

#[cfg(windows)]
lazy_static::lazy_static! {
    /// Windows Job Object handle. Child processes assigned to this job
    /// are automatically terminated when the job handle is closed (i.e., when Venus exits).
    static ref WINDOWS_JOB: Arc<Mutex<Option<WindowsJobObject>>> = Arc::new(Mutex::new(None));
}

#[cfg(windows)]
struct WindowsJobObject {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WindowsJobObject {
    fn create() -> Result<Self, std::io::Error> {
        use windows_sys::Win32::System::JobObjects::*;
        use windows_sys::Win32::Foundation::*;

        unsafe {
            // Create job object
            let job_handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job_handle == 0 {
                return Err(std::io::Error::last_os_error());
            }

            // Configure job to kill all processes when job handle is closed
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let result = SetInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if result == 0 {
                CloseHandle(job_handle);
                return Err(std::io::Error::last_os_error());
            }

            Ok(Self { handle: job_handle })
        }
    }

    fn assign_process(&self, process_handle: windows_sys::Win32::Foundation::HANDLE) -> Result<(), std::io::Error> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        unsafe {
            if AssignProcessToJobObject(self.handle, process_handle) == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsJobObject {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
/// Initialize the Windows job object. Called once on first LSP connection.
async fn ensure_windows_job() -> Result<(), std::io::Error> {
    let mut job = WINDOWS_JOB.lock().await;
    if job.is_none() {
        *job = Some(WindowsJobObject::create()?);
        tracing::info!("Created Windows job object for automatic process cleanup");
    }
    Ok(())
}

/// Register a rust-analyzer process for cleanup on shutdown.
async fn register_process(pid: u32) {
    let mut processes = ANALYZER_PROCESSES.lock().await;
    processes.push(pid);
    tracing::debug!("Registered rust-analyzer process: {}", pid);
}

/// Unregister a rust-analyzer process.
async fn unregister_process(pid: u32) {
    let mut processes = ANALYZER_PROCESSES.lock().await;
    processes.retain(|&p| p != pid);
    tracing::debug!("Unregistered rust-analyzer process: {}", pid);
}

/// Kill all registered rust-analyzer processes.
/// Called on server shutdown.
pub async fn kill_all_processes() {
    let mut processes = ANALYZER_PROCESSES.lock().await;

    for &pid in processes.iter() {
        tracing::info!("Killing rust-analyzer process: {}", pid);

        #[cfg(unix)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }

        #[cfg(windows)]
        {
            use std::process::Command as StdCommand;
            let _ = StdCommand::new("taskkill")
                .args(&["/PID", &pid.to_string(), "/F"])
                .output();
        }
    }

    processes.clear();
    tracing::info!("All rust-analyzer processes terminated");
}

/// Handle an LSP WebSocket connection.
pub async fn handle_lsp_websocket(socket: WebSocket, notebook_path: PathBuf) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Ensure rust-analyzer is available (download if needed)
    let ra_path = match rust_analyzer::ensure_available().await {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("Failed to get rust-analyzer: {}", e);
            let error_msg = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "window/showMessage",
                "params": {
                    "type": 1,
                    "message": format!("Failed to get rust-analyzer: {}. Please install it manually.", e)
                }
            });
            let _ = ws_sender
                .send(Message::Text(error_msg.to_string().into()))
                .await;
            return;
        }
    };

    // Start rust-analyzer
    let workspace_root = notebook_path
        .parent()
        .unwrap_or(&notebook_path)
        .to_path_buf();

    tracing::info!("Starting rust-analyzer from: {}", ra_path.display());

    // Build command with process group configuration
    let mut cmd = Command::new(&ra_path);
    cmd.current_dir(&workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // On Linux: Use prctl to kill child when parent dies
    #[cfg(target_os = "linux")]
    {
        #[allow(unused_imports)] // CommandExt trait is needed for pre_exec
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // PR_SET_PDEATHSIG = 1, SIGKILL = 9
                // This ensures rust-analyzer is killed if Venus crashes/is killed
                if libc::prctl(1, 9) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    // On other Unix: Create new process group for manual cleanup
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        #[allow(unused_imports)] // CommandExt trait is needed for pre_exec
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // Create new process group for manual cleanup
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    // On Windows: Ensure job object exists for automatic cleanup
    #[cfg(windows)]
    {
        if let Err(e) = ensure_windows_job().await {
            tracing::error!("Failed to create Windows job object: {}", e);
        }
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            tracing::error!("Failed to start rust-analyzer: {}", e);
            let error_msg = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "window/showMessage",
                "params": {
                    "type": 1,
                    "message": format!("Failed to start rust-analyzer: {}", e)
                }
            });
            let _ = ws_sender
                .send(Message::Text(error_msg.to_string().into()))
                .await;
            return;
        }
    };

    // Get process ID and register for cleanup
    let pid = child.id().expect("Failed to get process ID");
    register_process(pid).await;

    // On Windows: Assign process to job object for automatic cleanup
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let job = WINDOWS_JOB.lock().await;
        if let Some(job_obj) = job.as_ref() {
            let handle = child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
            if let Err(e) = job_obj.assign_process(handle) {
                tracing::warn!("Failed to assign rust-analyzer to job object: {}", e);
            } else {
                tracing::debug!("Assigned rust-analyzer (PID {}) to Windows job object", pid);
            }
        }
    }

    let stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let stderr = child.stderr.take().expect("Failed to get stderr");

    let stdin = Arc::new(Mutex::new(stdin));
    let stdin_clone = stdin.clone();

    // Task: Forward WebSocket messages to rust-analyzer stdin
    let ws_to_lsp = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // LSP requires Content-Length header
                    let content = text.as_str();
                    let header = format!("Content-Length: {}\r\n\r\n", content.len());

                    let mut stdin = stdin_clone.lock().await;
                    if stdin.write_all(header.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin.write_all(content.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin.flush().await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }
    });

    // Wrap sender in Arc<Mutex> for sharing
    let ws_sender = Arc::new(Mutex::new(ws_sender));
    let ws_sender_clone = ws_sender.clone();

    // Task: Forward rust-analyzer stdout to WebSocket
    let lsp_to_ws = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut header_buf = String::new();

        loop {
            header_buf.clear();

            // Read Content-Length header
            if reader.read_line(&mut header_buf).await.is_err() {
                break;
            }

            if header_buf.is_empty() {
                break;
            }

            // Parse Content-Length
            let content_length: usize = if header_buf.starts_with("Content-Length:") {
                header_buf
                    .trim_start_matches("Content-Length:")
                    .trim()
                    .parse()
                    .unwrap_or(0)
            } else {
                continue;
            };

            // Skip empty line after header
            header_buf.clear();
            if reader.read_line(&mut header_buf).await.is_err() {
                break;
            }

            // Read content
            let mut content = vec![0u8; content_length];
            if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut content)
                .await
                .is_err()
            {
                break;
            }

            // Send to WebSocket
            if let Ok(text) = String::from_utf8(content) {
                let mut sender = ws_sender_clone.lock().await;
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Task: Log stderr
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();

        while reader.read_line(&mut line).await.is_ok() {
            if line.is_empty() {
                break;
            }
            tracing::debug!("rust-analyzer stderr: {}", line.trim());
            line.clear();
        }
    });

    // Wait for BOTH tasks to complete (not just first one)
    // This prevents orphaned tasks from continuing to run
    let _ = tokio::join!(ws_to_lsp, lsp_to_ws);

    // Clean up stderr task
    stderr_task.abort();
    let _ = stderr_task.await;

    // Kill rust-analyzer process
    let _ = child.kill().await;

    // Unregister from cleanup list
    unregister_process(pid).await;

    tracing::info!("LSP session ended");
}
