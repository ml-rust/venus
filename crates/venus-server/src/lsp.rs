//! LSP (Language Server Protocol) proxy for rust-analyzer.
//!
//! Provides a WebSocket endpoint that proxies LSP messages between
//! the Monaco editor frontend and a rust-analyzer instance.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// LSP proxy that manages a rust-analyzer instance.
pub struct LspProxy {
    /// Path to the notebook file (for workspace root).
    notebook_path: PathBuf,
    /// rust-analyzer process.
    process: Option<Child>,
}

impl LspProxy {
    /// Create a new LSP proxy for a notebook.
    pub fn new(notebook_path: PathBuf) -> Self {
        Self {
            notebook_path,
            process: None,
        }
    }

    /// Start the rust-analyzer process.
    pub async fn start(&mut self) -> Result<(), std::io::Error> {
        let workspace_root = self
            .notebook_path
            .parent()
            .unwrap_or(&self.notebook_path)
            .to_path_buf();

        tracing::info!("Starting rust-analyzer for workspace: {}", workspace_root.display());

        let child = Command::new("rust-analyzer")
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.process = Some(child);
        Ok(())
    }

    /// Stop the rust-analyzer process.
    pub async fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }
    }
}

/// Handle an LSP WebSocket connection.
pub async fn handle_lsp_websocket(socket: WebSocket, notebook_path: PathBuf) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Start rust-analyzer
    let workspace_root = notebook_path
        .parent()
        .unwrap_or(&notebook_path)
        .to_path_buf();

    let mut child = match Command::new("rust-analyzer")
        .current_dir(&workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
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

    // Wait for tasks to complete
    tokio::select! {
        _ = ws_to_lsp => {},
        _ = lsp_to_ws => {},
    }

    // Clean up
    stderr_task.abort();

    // Kill rust-analyzer process
    let _ = child.kill().await;

    tracing::info!("LSP session ended");
}
