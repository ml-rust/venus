//! Venus interactive notebook server.
//!
//! Provides a WebSocket server for real-time notebook interaction.
//!
//! # Architecture
//!
//! The server consists of:
//! - **Session**: Manages notebook state, compilation, and execution
//! - **Protocol**: Defines client/server message types
//! - **Routes**: HTTP and WebSocket handlers
//! - **Watcher**: File system monitoring for external changes
//!
//! # Features
//!
//! - `embedded-frontend` (default): Embeds the web UI for standalone use

#[cfg(feature = "embedded-frontend")]
pub mod embedded_frontend;
pub mod error;
pub mod lsp;
pub mod protocol;
pub mod routes;
pub mod rust_analyzer;
pub mod session;
pub mod undo;
pub mod watcher;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::{Mutex as TokioMutex, RwLock};

pub use error::{ServerError, ServerResult};
pub use protocol::{ClientMessage, ServerMessage};
pub use routes::{AppState, create_router};
pub use session::{NotebookSession, SessionHandle};
pub use watcher::{FileEvent, FileWatcher};

// Re-export LSP cleanup function
pub use lsp::kill_all_processes as kill_all_lsp_processes;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Host address to bind to.
    pub host: String,
    /// Port to listen on.
    pub port: u16,
    /// Whether to open browser on start.
    pub open_browser: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            open_browser: false,
        }
    }
}

/// Start the Venus server for a notebook.
pub async fn serve(notebook_path: impl AsRef<Path>, config: ServerConfig) -> ServerResult<()> {
    let path = notebook_path.as_ref();

    // Create shared interrupt flag (AtomicBool for lock-free access)
    let interrupted = Arc::new(AtomicBool::new(false));

    // Create session with shared interrupt flag
    let (session, _rx) = NotebookSession::new(path, interrupted.clone())?;

    // Get the kill handle from the executor - it's an Arc so it will see
    // updates when workers are spawned during execution
    let kill_handle = session.get_kill_handle();

    let session = Arc::new(RwLock::new(session));

    // Create app state with shared kill handle and interrupt flag
    let state = Arc::new(AppState {
        session: session.clone(),
        kill_handle: Arc::new(TokioMutex::new(kill_handle)),
        interrupted,
    });

    // Create router
    let app = create_router(state);

    // Create file watcher
    let mut watcher = FileWatcher::new(path)?;

    // Spawn watcher task and store handle for cleanup
    let watcher_task = tokio::spawn(async move {
        while let Some(event) = watcher.recv().await {
            match event {
                FileEvent::Modified(_) => {
                    // NOTE: We do NOT auto-reload here. External file changes should be picked up
                    // manually via "Restart Kernel" button. Auto-reloading causes infinite loops
                    // when editors perform frequent auto-saves or temporary file operations.
                    tracing::debug!("Notebook file changed externally (ignored, use Restart Kernel to apply)");
                }
                FileEvent::Removed(path) => {
                    tracing::warn!("Notebook file removed: {}", path.display());
                }
                FileEvent::Created(_) => {}
            }
        }
    });

    // Build address
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|_| ServerError::Io {
            path: std::path::PathBuf::new(),
            message: format!("Invalid address: {}:{}", config.host, config.port),
        })?;

    tracing::info!("Starting Venus server at http://{}", addr);

    // Open browser if requested
    if config.open_browser {
        tracing::info!("Open http://{} in your browser", addr);
    }

    // Start server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Create shutdown signal channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Handle Ctrl+C for graceful shutdown
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Received shutdown signal");
            let _ = shutdown_tx.send(());
        }
    });

    // Serve with graceful shutdown
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });

    server.await?;

    // Clean up file watcher task
    watcher_task.abort();
    let _ = watcher_task.await;

    tracing::info!("Server shutdown complete");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert!(!config.open_browser);
    }
}
