//! HTTP and WebSocket routes for Venus server.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{IntoResponse, Json},
    routing::get,
};

#[cfg(feature = "embedded-frontend")]
use axum::extract::Path as AxumPath;

#[cfg(not(feature = "embedded-frontend"))]
use axum::response::Html;
use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tower_http::cors::CorsLayer;
use venus_core::execute::ExecutorKillHandle;
use venus_core::graph::CellId;

use crate::lsp;
use crate::protocol::{CellState, ClientMessage, ServerMessage};
use crate::session::NotebookSession;

#[cfg(feature = "embedded-frontend")]
use crate::embedded_frontend;

// Re-export InterruptFlag from session module
pub use crate::session::InterruptFlag;

/// Application state shared across handlers.
pub struct AppState {
    /// Active notebook session.
    pub session: Arc<RwLock<NotebookSession>>,
    /// Kill handle for interrupting execution without holding session lock.
    /// This is separate from the session so interrupt can work even when
    /// execute_cell is holding the write lock.
    pub kill_handle: Arc<TokioMutex<Option<ExecutorKillHandle>>>,
    /// Flag to track if execution was interrupted by user.
    /// Uses AtomicBool so it can be checked without locks.
    pub interrupted: InterruptFlag,
}

/// Create the router with all routes.
pub fn create_router(state: Arc<AppState>) -> Router {
    let router = Router::new()
        .route("/health", get(health_handler))
        .route("/ws", get(ws_handler))
        .route("/lsp", get(lsp_handler))
        .route("/api/state", get(state_handler))
        .route("/api/graph", get(graph_handler));

    // Add frontend routes
    #[cfg(feature = "embedded-frontend")]
    let router = router
        .route("/", get(frontend_index_handler))
        .route("/static/{*path}", get(static_handler));

    #[cfg(not(feature = "embedded-frontend"))]
    let router = router.route("/", get(index_handler));

    router
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Index page handler (fallback when embedded-frontend is disabled).
#[cfg(not(feature = "embedded-frontend"))]
async fn index_handler() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Venus Notebook</title>
    <style>
        body { font-family: system-ui, sans-serif; margin: 2rem; }
        h1 { color: #7c3aed; }
        pre { background: #f3f4f6; padding: 1rem; border-radius: 0.5rem; }
    </style>
</head>
<body>
    <h1>Venus Notebook Server</h1>
    <p>WebSocket endpoint: <code>/ws</code></p>
    <p>API endpoints:</p>
    <ul>
        <li><code>GET /health</code> - Health check</li>
        <li><code>GET /api/state</code> - Current notebook state</li>
        <li><code>GET /api/graph</code> - Dependency graph</li>
    </ul>
    <p><em>Note: The full UI is available with the <code>embedded-frontend</code> feature.</em></p>
    <script>
        const ws = new WebSocket(`ws://${location.host}/ws`);
        ws.onmessage = (e) => console.log('Server:', JSON.parse(e.data));
        ws.onopen = () => ws.send(JSON.stringify({ type: 'get_state' }));
    </script>
</body>
</html>"#,
    )
}

/// Serve the embedded frontend index.html.
#[cfg(feature = "embedded-frontend")]
async fn frontend_index_handler() -> impl IntoResponse {
    embedded_frontend::serve_index()
}

/// Serve static assets from the embedded frontend.
#[cfg(feature = "embedded-frontend")]
async fn static_handler(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    embedded_frontend::serve_static(path)
}

/// Health check handler.
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Get current notebook state.
async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.read().await;
    let notebook_state = session.get_state();
    Json(notebook_state)
}

/// Get dependency graph.
async fn graph_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.read().await;

    // Get graph info from session state
    let state_msg = session.get_state();
    match state_msg {
        ServerMessage::NotebookState {
            execution_order, ..
        } => Json(serde_json::json!({
            "execution_order": execution_order
        })),
        _ => Json(serde_json::json!({})),
    }
}

/// WebSocket upgrade handler.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

/// LSP WebSocket upgrade handler.
async fn lsp_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let notebook_path = {
        let session = state.session.read().await;
        session.path().to_path_buf()
    };
    ws.on_upgrade(move |socket| lsp::handle_lsp_websocket(socket, notebook_path))
}

/// Handle WebSocket connection.
async fn handle_websocket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to server messages
    let mut rx = {
        let session = state.session.read().await;
        session.subscribe()
    };

    // Send initial state
    {
        let session = state.session.read().await;
        let initial_state = session.get_state();
        if let Ok(json) = serde_json::to_string(&initial_state) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
    }

    // Spawn task to forward server messages to client
    let sender = Arc::new(tokio::sync::Mutex::new(sender));
    let sender_clone = sender.clone();

    let forward_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                let mut sender = sender_clone.lock().await;
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming client messages
    while let Some(result) = receiver.next().await {
        match result {
            Ok(Message::Text(text)) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(msg) => handle_client_message(msg, &state, &sender).await,
                Err(e) => {
                    tracing::warn!("Failed to parse client message: {} (input: {})", e, text);
                    send_message(
                        &sender,
                        &ServerMessage::Error {
                            message: format!("Invalid message format: {}", e),
                        },
                    )
                    .await;
                }
            },
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::warn!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Ensure forward task terminates cleanly
    forward_task.abort();
    let _ = forward_task.await;
}

/// Send a server message through the WebSocket.
async fn send_message(
    sender: &Arc<tokio::sync::Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    msg: &ServerMessage,
) {
    if let Ok(json) = serde_json::to_string(msg) {
        let mut sender = sender.lock().await;
        let _ = sender.send(Message::Text(json.into())).await;
    }
}

/// Generic handler for cell operations following the DRY principle.
///
/// This eliminates code duplication across markdown, definition, and future cell types.
/// All cell operations follow the same pattern:
/// 1. Execute the operation on the session
/// 2. Send response message to the requesting client
/// 3. Broadcast updated state and undo/redo state to all clients (if successful)
///
/// # Arguments
/// * `session` - Mutable reference to the notebook session
/// * `operation` - Closure that performs the cell operation, returning Result<T>
/// * `response_constructor` - Function that constructs the appropriate ServerMessage from the result
/// * `sender` - WebSocket sender for client response
async fn handle_cell_operation<T, F, R>(
    session: &mut NotebookSession,
    operation: F,
    response_constructor: R,
    sender: &Arc<tokio::sync::Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
) where
    F: FnOnce(&mut NotebookSession) -> crate::error::ServerResult<T>,
    R: FnOnce(Result<T, String>) -> ServerMessage,
{
    let result = operation(session);

    // Convert Result<T, ServerError> to Result<T, String> for the response constructor
    match result {
        Ok(value) => {
            let msg = response_constructor(Ok(value));
            send_message(sender, &msg).await;

            // Broadcast updated state and undo/redo state to all clients
            let state_msg = session.get_state();
            session.broadcast(state_msg);
            let undo_state = session.get_undo_redo_state();
            session.broadcast(undo_state);
        }
        Err(e) => {
            let msg = response_constructor(Err(e.to_string()));
            send_message(sender, &msg).await;
        }
    };

    
}

/// Handle a client message.
async fn handle_client_message(
    msg: ClientMessage,
    state: &Arc<AppState>,
    sender: &Arc<tokio::sync::Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
) {
    match msg {
        ClientMessage::GetState => {
            let session = state.session.read().await;
            let state_msg = session.get_state();
            send_message(sender, &state_msg).await;
        }

        ClientMessage::ExecuteCell { cell_id } => {
            // Spawn execution in a separate task so interrupt messages can be processed
            let state_clone = state.clone();

            tokio::spawn(async move {
                // Get the kill handle before acquiring session lock
                {
                    let session = state_clone.session.read().await;
                    let kill_handle = session.get_kill_handle();
                    *state_clone.kill_handle.lock().await = kill_handle;
                }

                // Use spawn_blocking because execute_cell does synchronous IPC
                // which would otherwise block the tokio runtime
                let state_for_blocking = state_clone.clone();
                let exec_result = tokio::task::spawn_blocking(move || {
                    // We need to enter the tokio runtime context for async operations
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        let mut session = state_for_blocking.session.write().await;
                        session.execute_cell(cell_id).await
                    })
                }).await;

                // Clear kill handle after execution
                *state_clone.kill_handle.lock().await = None;

                // Session handles interrupt detection and sends appropriate messages.
                // We only need to handle unexpected task-level errors here.
                match exec_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        // Session already sent the appropriate message (CellError or ExecutionAborted)
                        tracing::debug!("Execution returned error (already handled by session): {}", e);
                    }
                    Err(e) => {
                        tracing::error!("Task join error: {}", e);
                    }
                }
            });
        }

        ClientMessage::ExecuteAll => {
            // Get kill handle BEFORE spawning - the Arc inside is shared with executor
            // so it will be updated when the worker actually starts
            {
                let session = state.session.read().await;
                let kill_handle = session.get_kill_handle();
                *state.kill_handle.lock().await = kill_handle;
            }

            // Spawn execution in a separate task so the WebSocket can still process messages
            let state_clone = state.clone();

            tokio::spawn(async move {
                // Use spawn_blocking because execute_all does synchronous IPC
                let state_for_blocking = state_clone.clone();
                let exec_result = tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        let mut session = state_for_blocking.session.write().await;
                        session.execute_all().await
                    })
                }).await;

                // Clear kill handle after execution
                *state_clone.kill_handle.lock().await = None;

                match exec_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::error!("Execution error: {}", e);
                    }
                    Err(e) => {
                        tracing::error!("Task join error: {}", e);
                    }
                }
            });
        }

        ClientMessage::ExecuteDirty => {
            // Get dirty cells and kill handle BEFORE spawning
            let dirty_cells = {
                let session = state.session.read().await;
                session.get_dirty_cell_ids()
            };

            // Get kill handle synchronously - the Arc inside is shared with executor
            {
                let session = state.session.read().await;
                let kill_handle = session.get_kill_handle();
                *state.kill_handle.lock().await = kill_handle;
            }

            // Spawn execution in a separate task so the WebSocket can still process messages
            let state_clone = state.clone();

            tokio::spawn(async move {
                // Use spawn_blocking for each cell execution
                for cell_id in dirty_cells {
                    let state_for_blocking = state_clone.clone();
                    let exec_result = tokio::task::spawn_blocking(move || {
                        let rt = tokio::runtime::Handle::current();
                        rt.block_on(async {
                            let mut session = state_for_blocking.session.write().await;
                            session.execute_cell(cell_id).await
                        })
                    }).await;

                    match exec_result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            tracing::error!("Execution error for {:?}: {}", cell_id, e);
                        }
                        Err(e) => {
                            tracing::error!("Task join error for {:?}: {}", cell_id, e);
                        }
                    }
                }

                // Clear kill handle after execution
                *state_clone.kill_handle.lock().await = None;
            });
        }

        ClientMessage::CellEdit { cell_id, .. } => {
            let mut session = state.session.write().await;
            session.mark_dirty(cell_id);
        }

        ClientMessage::Interrupt => {
            // Use the kill handle directly - doesn't need session lock!
            // This allows interrupt to work even while execute_cell holds the write lock.
            let kill_handle = state.kill_handle.lock().await;
            if let Some(ref handle) = *kill_handle {
                tracing::info!("Killing worker process via interrupt request");
                // Set interrupted flag so session shows friendly message instead of error
                state.interrupted.store(true, Ordering::SeqCst);
                handle.kill();
            } else {
                send_message(
                    sender,
                    &ServerMessage::Error {
                        message: "No execution in progress to abort".to_string(),
                    },
                )
                .await;
            }
        }

        ClientMessage::Sync => {
            let session = state.session.read().await;
            let rs_path = session.path();
            let ipynb_path = rs_path.with_extension("ipynb");

            match venus_sync::sync_to_ipynb(rs_path, &ipynb_path, None) {
                Ok(()) => {
                    send_message(
                        sender,
                        &ServerMessage::SyncCompleted {
                            ipynb_path: ipynb_path.display().to_string(),
                        },
                    )
                    .await;
                }
                Err(e) => {
                    tracing::error!("Sync error: {}", e);
                    send_message(
                        sender,
                        &ServerMessage::Error {
                            message: e.to_string(),
                        },
                    )
                    .await;
                }
            }
        }

        ClientMessage::GetGraph => {
            let session = state.session.read().await;
            let state_msg = session.get_state();
            send_message(sender, &state_msg).await;
        }

        ClientMessage::WidgetUpdate {
            cell_id,
            widget_id,
            value,
        } => {
            // Store the new widget value - does NOT trigger re-execution
            let mut session = state.session.write().await;
            session.update_widget_value(cell_id, widget_id, value);
            // No response needed - value is stored silently
        }

        ClientMessage::SelectHistory { cell_id, index } => {
            let mut session = state.session.write().await;

            let output = session.select_history_entry(cell_id, index);

            if let Some(output) = output {
                // Collect dirty cells
                let dirty_cells: Vec<CellId> = session.cell_states()
                    .iter()
                    .filter(|(_, s)| s.is_dirty())
                    .map(|(id, _)| *id)
                    .collect();

                let count = session.get_history_count(cell_id);

                session.broadcast(ServerMessage::HistorySelected {
                    cell_id,
                    index,
                    count,
                    output: Some(output),
                    dirty_cells,
                });
            }
        }

        ClientMessage::InsertCell { after_cell_id } => {
            let mut session = state.session.write().await;

            match session.insert_cell(after_cell_id) {
                Ok(new_name) => {
                    // Find the new cell's ID by name
                    let new_cell_id = session.cell_states()
                        .iter()
                        .find(|(_, s)| s.name().unwrap_or("") == new_name)
                        .map(|(id, _)| *id)
                        .unwrap_or(CellId::new(0));

                    // Send confirmation
                    send_message(sender, &ServerMessage::CellInserted {
                        cell_id: new_cell_id,
                        error: None,
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::CellInserted {
                        cell_id: CellId::new(0),
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::DeleteCell { cell_id } => {
            let mut session = state.session.write().await;

            match session.delete_cell(cell_id) {
                Ok(()) => {
                    // Send confirmation
                    send_message(sender, &ServerMessage::CellDeleted {
                        cell_id,
                        error: None,
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::CellDeleted {
                        cell_id,
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::DuplicateCell { cell_id } => {
            let mut session = state.session.write().await;

            match session.duplicate_cell(cell_id) {
                Ok(new_name) => {
                    // Find the new cell's ID by name
                    let new_cell_id = session.cell_states()
                        .iter()
                        .find(|(_, s)| s.name().unwrap_or("") == new_name)
                        .map(|(id, _)| *id)
                        .unwrap_or(CellId::new(0));

                    // Send confirmation
                    send_message(sender, &ServerMessage::CellDuplicated {
                        original_cell_id: cell_id,
                        new_cell_id,
                        error: None,
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::CellDuplicated {
                        original_cell_id: cell_id,
                        new_cell_id: CellId::new(0),
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::MoveCell { cell_id, direction } => {
            let mut session = state.session.write().await;

            match session.move_cell(cell_id, direction) {
                Ok(()) => {
                    // Send confirmation
                    send_message(sender, &ServerMessage::CellMoved {
                        cell_id,
                        error: None,
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::CellMoved {
                        cell_id,
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::Undo => {
            let mut session = state.session.write().await;

            match session.undo() {
                Ok(description) => {
                    // Send confirmation
                    send_message(sender, &ServerMessage::UndoResult {
                        success: true,
                        error: None,
                        description: Some(description),
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::UndoResult {
                        success: false,
                        error: Some(e.to_string()),
                        description: None,
                    }).await;
                }
            }
        }

        ClientMessage::Redo => {
            let mut session = state.session.write().await;

            match session.redo() {
                Ok(description) => {
                    // Send confirmation
                    send_message(sender, &ServerMessage::RedoResult {
                        success: true,
                        error: None,
                        description: Some(description),
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::RedoResult {
                        success: false,
                        error: Some(e.to_string()),
                        description: None,
                    }).await;
                }
            }
        }

        ClientMessage::RestartKernel => {
            let mut session = state.session.write().await;

            match session.restart_kernel() {
                Ok(()) => {
                    tracing::info!("Kernel restarted successfully");
                    // KernelRestarted message already broadcast by restart_kernel()
                }
                Err(e) => {
                    tracing::error!("Kernel restart failed: {}", e);
                    send_message(sender, &ServerMessage::KernelRestarted {
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::ClearOutputs => {
            let mut session = state.session.write().await;
            session.clear_outputs();
            tracing::info!("All cell outputs cleared");
            // OutputsCleared message already broadcast by clear_outputs()
        }

        ClientMessage::RenameCell { cell_id, new_display_name } => {
            let mut session = state.session.write().await;

            match session.rename_cell(cell_id, new_display_name.clone()) {
                Ok(()) => {
                    // Send confirmation
                    send_message(sender, &ServerMessage::CellRenamed {
                        cell_id,
                        new_display_name,
                        error: None,
                    }).await;

                    // Broadcast updated state and undo/redo state to all clients
                    let state_msg = session.get_state();
                    session.broadcast(state_msg);
                    let undo_state = session.get_undo_redo_state();
                    session.broadcast(undo_state);
                }
                Err(e) => {
                    send_message(sender, &ServerMessage::CellRenamed {
                        cell_id,
                        new_display_name,
                        error: Some(e.to_string()),
                    }).await;
                }
            }
        }

        ClientMessage::InsertMarkdownCell { content, after_cell_id } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| {
                    s.insert_markdown_cell(content, after_cell_id)?;
                    // Find the newly inserted markdown cell by looking at the last one
                    let new_cell_id = s.cell_states()
                        .iter()
                        .filter_map(|(id, state)| {
                            if matches!(state, CellState::Markdown { .. }) {
                                Some(*id)
                            } else {
                                None
                            }
                        })
                        .last()
                        .unwrap_or(CellId::new(0));
                    Ok(new_cell_id)
                },
                |result| match result {
                    Ok(cell_id) => ServerMessage::MarkdownCellInserted {
                        cell_id,
                        error: None,
                    },
                    Err(e) => ServerMessage::MarkdownCellInserted {
                        cell_id: CellId::new(0),
                        error: Some(e),
                    },
                },
                sender,
            ).await;
        }

        ClientMessage::EditMarkdownCell { cell_id, new_content } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.edit_markdown_cell(cell_id, new_content),
                |result| ServerMessage::MarkdownCellEdited {
                    cell_id,
                    error: result.err(),
                },
                sender,
            ).await;
        }

        ClientMessage::DeleteMarkdownCell { cell_id } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.delete_markdown_cell(cell_id),
                |result| ServerMessage::MarkdownCellDeleted {
                    cell_id,
                    error: result.err(),
                },
                sender,
            ).await;
        }

        ClientMessage::MoveMarkdownCell { cell_id, direction } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.move_markdown_cell(cell_id, direction),
                |result| ServerMessage::MarkdownCellMoved {
                    cell_id,
                    error: result.err(),
                },
                sender,
            ).await;
        }

        ClientMessage::InsertDefinitionCell { content, definition_type, after_cell_id } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.insert_definition_cell(content, definition_type, after_cell_id),
                |result| match result {
                    Ok(cell_id) => ServerMessage::DefinitionCellInserted {
                        cell_id,
                        error: None,
                    },
                    Err(e) => ServerMessage::DefinitionCellInserted {
                        cell_id: CellId::new(0),
                        error: Some(e),
                    },
                },
                sender,
            ).await;
        }

        ClientMessage::EditDefinitionCell { cell_id, new_content } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.edit_definition_cell(cell_id, new_content),
                |result| match result {
                    Ok(dirty_cells) => ServerMessage::DefinitionCellEdited {
                        cell_id,
                        error: None,
                        dirty_cells,
                    },
                    Err(e) => ServerMessage::DefinitionCellEdited {
                        cell_id,
                        error: Some(e),
                        dirty_cells: vec![],
                    },
                },
                sender,
            ).await;
        }

        ClientMessage::DeleteDefinitionCell { cell_id } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.delete_definition_cell(cell_id),
                |result| ServerMessage::DefinitionCellDeleted {
                    cell_id,
                    error: result.err(),
                },
                sender,
            ).await;
        }

        ClientMessage::MoveDefinitionCell { cell_id, direction } => {
            let mut session = state.session.write().await;

            handle_cell_operation(
                &mut session,
                |s| s.move_definition_cell(cell_id, direction),
                |result| ServerMessage::DefinitionCellMoved {
                    cell_id,
                    error: result.err(),
                },
                sender,
            ).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_health_json() {
        let health = serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION")
        });
        assert_eq!(health["status"], "ok");
    }
}
