/**
 * Venus Notebook Frontend Application
 *
 * Handles WebSocket communication, cell rendering, and user interactions.
 */

// Configure Monaco editor loader
require.config({ paths: { 'vs': 'https://cdn.jsdelivr.net/npm/monaco-editor@0.45.0/min/vs' }});

// Global state
const state = {
    ws: null,
    connected: false,
    reconnectAttempts: 0,
    maxReconnectAttempts: 10,
    reconnectDelay: 1000,
    cells: new Map(),
    sourceOrder: [],      // Order cells appear in .rs file (for display)
    executionOrder: [],   // Topological order (for execution)
    editors: new Map(),
    monacoReady: false,
    // graphVisible: false,  // Hidden (plotr in development)
    notebookPath: '',
    workspaceRoot: null,  // Workspace root directory (from server)
    cargoTomlPath: null,  // Path to Cargo.toml (from server)
    executing: false,  // Track if any execution is in progress
    runningCellId: null,  // Track currently running cell
    executionHistory: new Map(),  // Map<cellId, Array<HistoryEntry>>
    canUndo: false,
    canRedo: false,
    undoDescription: null,
    redoDescription: null,
};

// DOM Elements
const elements = {
    notebookPath: document.getElementById('notebook-path'),
    connectionStatus: document.getElementById('connection-status'),
    cellsContainer: document.getElementById('cells-container'),
    runAllBtn: document.getElementById('run-all-btn'),
    syncBtn: document.getElementById('sync-btn'),
    // Graph elements hidden (plotr in development)
    // graphToggleBtn: document.getElementById('graph-toggle-btn'),
    // graphPanel: document.getElementById('graph-panel'),
    // graphCloseBtn: document.getElementById('graph-close-btn'),
    // graphContainer: document.getElementById('graph-container'),
    cellCount: document.getElementById('cell-count'),
    executionTime: document.getElementById('execution-time'),
    toastContainer: document.getElementById('toast-container'),
    variableExplorer: document.getElementById('variable-explorer'),
    explorerContent: document.getElementById('explorer-content'),
    explorerToggleBtn: document.getElementById('explorer-toggle-btn'),
    variablesToggleBtn: document.getElementById('variables-toggle-btn'),
    restartKernelBtn: document.getElementById('restart-kernel-btn'),
    clearOutputsBtn: document.getElementById('clear-outputs-btn')
};

// Centralized SVG icons for consistency and maintainability
const ICONS = {
    play: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>',
    save: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M17 3H5c-1.11 0-2 .9-2 2v14c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V7l-4-4zm-5 16c-1.66 0-3-1.34-3-3s1.34-3 3-3 3 1.34 3 3-1.34 3-3 3zm3-10H5V5h10v4z"/></svg>',
    chevronLeft: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M15.41 7.41L14 6l-6 6 6 6 1.41-1.41L10.83 12z"/></svg>',
    chevronRight: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8.59 16.59L10 18l6-6-6-6-1.41 1.41L13.17 12z"/></svg>',
    chevronLeftSmall: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15.41 7.41L14 6l-6 6 6 6 1.41-1.41L10.83 12z"/></svg>',
    chevronRightSmall: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M8.59 16.59L10 18l6-6-6-6-1.41 1.41L13.17 12z"/></svg>',
};

// Memory limits for execution history
const HISTORY_CONFIG = {
    maxEntriesPerCell: 10,
    maxOutputSize: 100 * 1024,  // 100KB per output
    maxTotalSize: 5 * 1024 * 1024,  // 5MB total history
};

// Initialize Monaco
require(['vs/editor/editor.main'], function() {
    state.monacoReady = true;

    // Define Rust language configuration FIRST
    monaco.languages.register({ id: 'rust' });

    // Set editor theme BEFORE creating editors
    monaco.editor.defineTheme('venus-dark', {
        base: 'vs-dark',
        inherit: true,
        rules: [
            { token: 'comment', foreground: '6e7681', fontStyle: 'italic' },
            { token: 'keyword', foreground: 'ff7b72' },
            { token: 'string', foreground: 'a5d6ff' },
            { token: 'number', foreground: '79c0ff' },
            { token: 'type', foreground: 'ffa657' },
            { token: 'function', foreground: 'd2a8ff' },
            { token: 'variable', foreground: 'ffa657' },
            { token: 'operator', foreground: 'ff7b72' },
        ],
        colors: {
            'editor.background': '#1c2128',
            'editor.foreground': '#e6edf3',
            'editor.lineHighlightBackground': '#21262d',
            'editorLineNumber.foreground': '#6e7681',
            'editorLineNumber.activeForeground': '#e6edf3',
            'editor.selectionBackground': '#264f78',
            'editorCursor.foreground': '#a78bfa',
        }
    });

    // Create any pending editors that were waiting for Monaco to load
    state.cells.forEach((cell, cellId) => {
        if (!state.editors.has(cellId)) {
            const isDefinition = cell.cell_type === 'definition';
            const source = isDefinition ? (cell.content || '') : (cell.source || '');
            if (source) {
                createEditor(cellId, source);
            }
        }
    });
});

// =====================================
// WebSocket Connection
// =====================================

function connect() {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${location.host}/ws`;

    state.ws = new WebSocket(wsUrl);

    state.ws.onopen = () => {
        state.connected = true;
        state.reconnectAttempts = 0;
        updateConnectionStatus('connected');
        console.log('WebSocket connected');
    };

    state.ws.onmessage = (event) => {
        try {
            const msg = JSON.parse(event.data);
            handleServerMessage(msg);
        } catch (e) {
            console.error('Failed to parse message:', e);
        }
    };

    state.ws.onclose = () => {
        state.connected = false;
        updateConnectionStatus('disconnected');
        console.log('WebSocket disconnected');
        scheduleReconnect();
    };

    state.ws.onerror = (error) => {
        console.error('WebSocket error:', error);
    };
}

function scheduleReconnect() {
    if (state.reconnectAttempts >= state.maxReconnectAttempts) {
        showToast('Connection lost. Please refresh the page.', 'error');
        return;
    }

    state.reconnectAttempts++;
    const delay = state.reconnectDelay * Math.pow(1.5, state.reconnectAttempts - 1);

    console.log(`Reconnecting in ${delay}ms (attempt ${state.reconnectAttempts})`);
    setTimeout(connect, delay);
}

function send(msg) {
    if (state.connected && state.ws) {
        state.ws.send(JSON.stringify(msg));
    }
}

function updateConnectionStatus(status) {
    elements.connectionStatus.className = `connection-status ${status}`;
    const statusText = elements.connectionStatus.querySelector('.status-text');
    switch (status) {
        case 'connected':
            statusText.textContent = 'Connected';
            break;
        case 'disconnected':
            statusText.textContent = 'Reconnecting...';
            break;
        default:
            statusText.textContent = 'Connecting...';
    }
}

// =====================================
// Message Handlers
// =====================================

function handleServerMessage(msg) {
    switch (msg.type) {
        case 'notebook_state':
            handleNotebookState(msg);
            break;
        case 'cell_started':
            handleCellStarted(msg);
            break;
        case 'cell_completed':
            handleCellCompleted(msg);
            break;
        case 'cell_error':
            handleCellError(msg);
            break;
        case 'compile_error':
            handleCompileError(msg);
            break;
        // Graph hidden (plotr in development)
        // case 'graph_updated':
        //     handleGraphUpdated(msg);
        //     break;
        case 'cell_inserted':
            handleCellInserted(msg);
            break;
        case 'cell_deleted':
            handleCellDeleted(msg);
            break;
        case 'cell_duplicated':
            handleCellDuplicated(msg);
            break;
        case 'cell_moved':
            handleCellMoved(msg);
            break;
        case 'markdown_cell_inserted':
            handleMarkdownCellInserted(msg);
            break;
        case 'markdown_cell_edited':
            handleMarkdownCellEdited(msg);
            break;
        case 'markdown_cell_deleted':
            handleMarkdownCellDeleted(msg);
            break;
        case 'markdown_cell_moved':
            handleMarkdownCellMoved(msg);
            break;
        case 'definition_cell_inserted':
            handleDefinitionCellInserted(msg);
            break;
        case 'definition_cell_edited':
            handleDefinitionCellEdited(msg);
            break;
        case 'definition_cell_deleted':
            handleDefinitionCellDeleted(msg);
            break;
        case 'definition_cell_moved':
            handleDefinitionCellMoved(msg);
            break;
        case 'undo_result':
            handleUndoResult(msg);
            break;
        case 'redo_result':
            handleRedoResult(msg);
            break;
        case 'undo_redo_state':
            handleUndoRedoState(msg);
            break;
        case 'file_changed':
            handleFileChanged(msg);
            break;
        case 'sync_completed':
            handleSyncCompleted(msg);
            break;
        case 'execution_aborted':
            handleExecutionAborted(msg);
            break;
        case 'history_selected':
            handleHistorySelected(msg);
            break;
        case 'kernel_restarted':
            handleKernelRestarted(msg);
            break;
        case 'outputs_cleared':
            handleOutputsCleared(msg);
            break;
        case 'error':
            showToast(msg.message, 'error');
            break;
        default:
            console.log('Unknown message type:', msg.type);
    }
}

function handleNotebookState(msg) {
    state.notebookPath = msg.path;
    state.workspaceRoot = msg.workspace_root || null;
    state.cargoTomlPath = msg.cargo_toml_path || null;
    state.sourceOrder = msg.source_order || msg.execution_order;  // Fallback for compatibility
    state.executionOrder = msg.execution_order;

    elements.notebookPath.textContent = msg.path;

    // Clear existing cells
    state.cells.clear();
    state.editors.forEach(editor => editor.dispose());
    state.editors.clear();

    // Store cells
    msg.cells.forEach(cell => {
        state.cells.set(cell.id, cell);
    });

    // Render cells
    renderCells();
    updateCellCount();
    renderVariableExplorer();

    // Graph hidden (plotr in development)
    // if (state.graphVisible && typeof renderGraph === 'function') {
    //     renderGraph(state.cells, msg.execution_order);
    // }
}

function handleCellStarted(msg) {
    const cell = state.cells.get(msg.cell_id);
    if (cell) {
        cell.status = 'running';
        state.executing = true;
        state.runningCellId = msg.cell_id;
        updateCellStatus(msg.cell_id);
        updateVariableItem(msg.cell_id);
        updateExecutionUI();
    }
}

function handleCellCompleted(msg) {
    const cell = state.cells.get(msg.cell_id);
    if (cell) {
        cell.status = 'success';
        cell.output = msg.output;
        cell.dirty = false;

        // Add to execution history
        addToHistory(msg.cell_id, {
            output: msg.output,
            error: null,
            duration: msg.duration_ms,
            source: cell.source,
        });

        // Clear execution state if this was the running cell
        if (state.runningCellId === msg.cell_id) {
            state.executing = false;
            state.runningCellId = null;
            updateExecutionUI();
        }

        updateCellStatus(msg.cell_id);
        updateCellOutput(msg.cell_id);
        updateVariableItem(msg.cell_id);
        updateHistoryControls(msg.cell_id);

        if (msg.duration_ms !== undefined) {
            updateCellTiming(msg.cell_id, msg.duration_ms);
        }
    }
}

function handleCellError(msg) {
    const cell = state.cells.get(msg.cell_id);
    if (cell) {
        cell.status = 'error';
        cell.error = { message: msg.error, location: msg.location };

        // Add to execution history
        addToHistory(msg.cell_id, {
            output: null,
            error: { message: msg.error, location: msg.location },
            duration: null,
            source: cell.source,
        });

        // Clear execution state if this was the running cell
        if (state.runningCellId === msg.cell_id) {
            state.executing = false;
            state.runningCellId = null;
            updateExecutionUI();
        }

        updateCellStatus(msg.cell_id);
        updateCellError(msg.cell_id);
        updateVariableItem(msg.cell_id);
        updateHistoryControls(msg.cell_id);
    }
}

function handleCompileError(msg) {
    const cell = state.cells.get(msg.cell_id);
    if (cell) {
        cell.status = 'error';
        cell.compileErrors = msg.errors;
        updateCellStatus(msg.cell_id);
        updateCellCompileErrors(msg.cell_id);
    }
}

// Graph hidden (plotr in development)
// function handleGraphUpdated(msg) {
//     if (state.graphVisible && typeof renderGraph === 'function') {
//         renderGraph(state.cells, state.executionOrder, msg.edges, msg.levels);
//     }
// }

function handleCellInserted(msg) {
    if (msg.error) {
        showToast(`Failed to insert cell: ${msg.error}`, 'error');
    } else {
        showToast('Cell added', 'success');
        // The notebook_state message will follow to update the UI
    }
}

function handleCellDeleted(msg) {
    if (msg.error) {
        showToast(`Failed to delete cell: ${msg.error}`, 'error');
    } else {
        showToast('Cell deleted', 'success');
        // The notebook_state message will follow to update the UI
    }
}

function handleCellDuplicated(msg) {
    if (msg.error) {
        showToast(`Failed to duplicate cell: ${msg.error}`, 'error');
    } else {
        showToast('Cell duplicated', 'success');
        // The notebook_state message will follow to update the UI
    }
}

function handleCellMoved(msg) {
    if (msg.error) {
        showToast(`Failed to move cell: ${msg.error}`, 'error');
    }
    // No success toast - the visual reorder is feedback enough
    // The notebook_state message will follow to update the UI
}

/**
 * Generic handler for cell operation results following DRY principle.
 *
 * Consolidates duplicate code across markdown and definition cell handlers.
 * Provides consistent error handling and user feedback across all cell types.
 *
 * @param {Object} msg - Server message containing operation result
 * @param {string} cellType - Type of cell ('markdown' or 'definition')
 * @param {string} operation - Operation performed ('inserted', 'edited', 'deleted', 'moved')
 */
function handleCellOperationResult(msg, cellType, operation) {
    // Capitalize first letter for display
    const displayType = cellType.charAt(0).toUpperCase() + cellType.slice(1);

    if (msg.error) {
        showToast(`Failed to ${operation.replace('ed', '')} ${cellType} cell: ${msg.error}`, 'error');
        return;
    }

    // Special handling for 'moved' operations - visual feedback is sufficient
    if (operation === 'moved') {
        // No success toast - the visual reorder is feedback enough
        return;
    }

    // Special handling for definition cell edits with dirty cells
    if (cellType === 'definition' && operation === 'edited' && msg.dirty_cells && msg.dirty_cells.length > 0) {
        showToast(`Definition updated - ${msg.dirty_cells.length} cell(s) marked dirty`, 'warning');
        return;
    }

    // Standard success messages
    const operationMessages = {
        inserted: 'added',
        edited: 'updated',
        deleted: 'deleted'
    };

    const successMessage = operationMessages[operation] || operation;
    showToast(`${displayType} cell ${successMessage}`, 'success');
    // The notebook_state message will follow to update the UI
}

function handleMarkdownCellInserted(msg) {
    handleCellOperationResult(msg, 'markdown', 'inserted');
}

function handleMarkdownCellEdited(msg) {
    handleCellOperationResult(msg, 'markdown', 'edited');
}

function handleMarkdownCellDeleted(msg) {
    handleCellOperationResult(msg, 'markdown', 'deleted');
}

function handleMarkdownCellMoved(msg) {
    handleCellOperationResult(msg, 'markdown', 'moved');
}

function handleDefinitionCellInserted(msg) {
    handleCellOperationResult(msg, 'definition', 'inserted');
}

function handleDefinitionCellEdited(msg) {
    handleCellOperationResult(msg, 'definition', 'edited');
}

function handleDefinitionCellDeleted(msg) {
    handleCellOperationResult(msg, 'definition', 'deleted');
}

function handleDefinitionCellMoved(msg) {
    handleCellOperationResult(msg, 'definition', 'moved');
}

function handleUndoResult(msg) {
    if (msg.success) {
        if (msg.description) {
            showToast(`Undo: ${msg.description}`, 'info');
        }
    } else if (msg.error) {
        showToast(`Undo failed: ${msg.error}`, 'error');
    }
}

function handleRedoResult(msg) {
    if (msg.success) {
        if (msg.description) {
            showToast(`Redo: ${msg.description}`, 'info');
        }
    } else if (msg.error) {
        showToast(`Redo failed: ${msg.error}`, 'error');
    }
}

function handleUndoRedoState(msg) {
    state.canUndo = msg.can_undo;
    state.canRedo = msg.can_redo;
    state.undoDescription = msg.undo_description;
    state.redoDescription = msg.redo_description;
    updateUndoRedoButtons();
}

function updateUndoRedoButtons() {
    const undoBtn = document.getElementById('undo-btn');
    const redoBtn = document.getElementById('redo-btn');

    if (undoBtn) {
        undoBtn.disabled = !state.canUndo;
        undoBtn.title = state.undoDescription ? `Undo: ${state.undoDescription}` : 'Undo (Ctrl+Z)';
    }
    if (redoBtn) {
        redoBtn.disabled = !state.canRedo;
        redoBtn.title = state.redoDescription ? `Redo: ${state.redoDescription}` : 'Redo (Ctrl+Shift+Z)';
    }
}

function handleFileChanged(msg) {
    showToast('Notebook file changed. Reloading...', 'info');
    // Request fresh state
    send({ type: 'get_state' });
}

function handleSyncCompleted(msg) {
    showToast(`Exported to ${msg.ipynb_path}`, 'success');
}

function handleExecutionAborted(msg) {
    // Reset the interrupted cell status and show interrupted message
    const cellId = msg.cell_id !== undefined ? msg.cell_id : state.runningCellId;
    if (cellId !== null) {
        const cell = state.cells.get(cellId);
        if (cell) {
            cell.status = 'idle';
            cell.error = null;
            cell.output = null;  // Clear previous output
            updateCellStatus(cellId);
            updateVariableItem(cellId);
            // Show interrupted message in output area
            const outputEl = document.getElementById(`output-${cellId}`);
            if (outputEl) {
                outputEl.style.display = 'block';
                outputEl.innerHTML = '<div class="output-interrupted">Execution interrupted</div>';
            }
        }
    }

    state.executing = false;
    state.runningCellId = null;
    updateExecutionUI();
}

function handleKernelRestarted(msg) {
    if (msg.error) {
        showToast(`Kernel restart failed: ${msg.error}`, 'error');
    } else {
        showToast('Kernel restarted successfully', 'success');
    }
    // NotebookState message will follow and update the UI
}

function handleOutputsCleared(msg) {
    if (msg.error) {
        showToast(`Clear outputs failed: ${msg.error}`, 'error');
    } else {
        showToast('All outputs cleared', 'success');
    }
    // NotebookState message will follow and update the UI
}

function handleHistorySelected(msg) {
    const { cell_id, index, count, output, dirty_cells } = msg;

    // Update the cell's output
    const cell = state.cells.get(cell_id);
    if (cell && output) {
        cell.output = output;
        cell.historyIndex = index;
        updateCellOutput(cell_id);
        updateVariableItem(cell_id);
    }

    // Mark dirty cells
    for (const dirtyCellId of dirty_cells) {
        const dirtyCell = state.cells.get(dirtyCellId);
        if (dirtyCell) {
            dirtyCell.dirty = true;
            updateCellStatus(dirtyCellId);
        }
    }

    // Show toast if cells became dirty
    if (dirty_cells.length > 0) {
        showToast(`${dirty_cells.length} dependent cell(s) need re-execution`, 'info');
    }
}

function updateExecutionUI() {
    const runAllBtn = elements.runAllBtn;

    if (state.executing) {
        // Change Run All to Stop
        runAllBtn.innerHTML = `
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                <rect x="6" y="6" width="12" height="12" rx="1"/>
            </svg>
            Stop
        `;
        runAllBtn.classList.remove('btn-primary');
        runAllBtn.classList.add('btn-danger');
        runAllBtn.onclick = interruptExecution;
    } else {
        // Restore Run All
        runAllBtn.innerHTML = `
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                <path d="M8 5v14l11-7z"/>
            </svg>
            Run All
        `;
        runAllBtn.classList.remove('btn-danger');
        runAllBtn.classList.add('btn-primary');
        runAllBtn.onclick = executeAll;
    }
}

// =====================================
// Cell Rendering
// =====================================

function renderCells() {
    elements.cellsContainer.innerHTML = '';

    if (state.cells.size === 0) {
        elements.cellsContainer.innerHTML = `
            <div class="empty-state">
                <div class="empty-state-icon">📓</div>
                <h2>No cells found</h2>
                <p>Create cells using #[venus::cell] attribute in your notebook</p>
            </div>
        `;
        return;
    }

    // Render cells in source file order
    state.sourceOrder.forEach(cellId => {
        const cell = state.cells.get(cellId);
        if (cell) {
            const cellEl = createCellElement(cell);
            elements.cellsContainer.appendChild(cellEl);
        }
    });

    // Add "Add Cell" buttons at the bottom
    const addCellDiv = document.createElement('div');
    addCellDiv.className = 'add-cell-container';
    addCellDiv.innerHTML = `
        <button class="btn btn-add-cell" data-action="insert-cell-end" title="Add new code cell">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/>
            </svg>
            Add Code Cell
        </button>
        <button class="btn btn-add-markdown" data-action="insert-markdown-end" title="Add new markdown cell">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/>
            </svg>
            Add Markdown Cell
        </button>
    `;
    elements.cellsContainer.appendChild(addCellDiv);
}

function createCellElement(cell) {
    // Route to appropriate renderer based on cell type
    if (cell.cell_type === 'markdown') {
        return createMarkdownCellElement(cell);
    } else {
        // Both code and definition cells use the same component
        return createCodeCellElement(cell);
    }
}

function createMarkdownCellElement(cell) {
    const div = document.createElement('div');
    div.className = 'cell cell-markdown';
    div.id = `cell-${cell.id}`;
    div.dataset.cellId = cell.id;
    div.dataset.cellType = 'markdown';

    // Render markdown content
    const contentHtml = typeof marked !== 'undefined'
        ? marked.parse(cell.content)
        : escapeHtml(cell.content);

    div.innerHTML = `
        <div class="markdown-cell-header">
            <div class="markdown-actions">
                <button class="btn btn-icon btn-edit-markdown" data-cell-id="${cell.id}" data-action="edit-markdown" title="Edit markdown">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04c.39-.39.39-1.02 0-1.41l-2.34-2.34c-.39-.39-1.02-.39-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-insert" data-cell-id="${cell.id}" data-action="insert-markdown" title="Insert markdown below">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-copy" data-cell-id="${cell.id}" data-action="copy-markdown" title="Copy markdown">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-move" data-cell-id="${cell.id}" data-action="move-markdown-up" title="Move up">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6-6 6z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-move" data-cell-id="${cell.id}" data-action="move-markdown-down" title="Move down">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M7.41 8.59L12 13.17l4.59-4.58L18 10l-6 6-6-6z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-delete" data-cell-id="${cell.id}" data-action="delete-markdown" title="Delete">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z"/>
                    </svg>
                </button>
            </div>
        </div>
        <div class="markdown-content" id="markdown-preview-${cell.id}">
            ${contentHtml}
        </div>
        <div class="markdown-editor-container" id="markdown-editor-${cell.id}" style="display: none;">
            <textarea class="markdown-textarea" id="markdown-textarea-${cell.id}">${escapeHtml(cell.content)}</textarea>
            <div class="markdown-editor-actions">
                <button class="btn btn-primary" data-cell-id="${cell.id}" data-action="save-markdown">Save</button>
                <button class="btn" data-cell-id="${cell.id}" data-action="cancel-markdown">Cancel</button>
            </div>
        </div>
    `;

    return div;
}

function createCodeCellElement(cell) {
    const isDefinition = cell.cell_type === 'definition';

    const div = document.createElement('div');
    div.className = `cell ${isDefinition ? 'cell-definition' : cell.status}`;
    div.id = `cell-${cell.id}`;
    div.dataset.cellId = cell.id;
    div.dataset.cellType = cell.cell_type || 'code';

    // Dependencies display (only for code cells)
    const depsHtml = !isDefinition && cell.dependencies && cell.dependencies.length > 0
        ? `<div class="cell-dependencies">
            <span>deps:</span>
            ${cell.dependencies.map(d => `<span class="cell-dep">${d}</span>`).join('')}
           </div>`
        : '';

    // Description (markdown) - fallback to plain text if marked isn't loaded
    const descHtml = cell.description
        ? `<div class="cell-description">${typeof marked !== 'undefined' ? marked.parse(cell.description) : escapeHtml(cell.description)}</div>`
        : '';

    // Doc comment for definition cells
    const docHtml = isDefinition && cell.doc_comment
        ? `<div class="cell-description">${typeof marked !== 'undefined' ? marked.parse(cell.doc_comment) : escapeHtml(cell.doc_comment)}</div>`
        : '';

    // Status display (only for code cells)
    const statusHtml = !isDefinition ? getStatusHtml(cell.status) : '';

    // RUN vs SAVE button
    const actionButton = isDefinition
        ? `<button class="btn btn-save" data-cell-id="${cell.id}" data-action="save-definition" title="Save Definition">
               ${ICONS.save}
           </button>`
        : `<button class="btn btn-run" data-cell-id="${cell.id}" data-action="run-cell" title="Run Cell">
               ${ICONS.play}
           </button>`;

    // Display name
    const displayName = isDefinition
        ? (cell.definition_type || 'definition').replace('_', ' ').toUpperCase()
        : cell.display_name;

    // Return type (only for code cells)
    const returnTypeHtml = !isDefinition
        ? `<span class="cell-type">→ ${cell.return_type}</span>`
        : '';

    div.innerHTML = `
        <div class="cell-header">
            <div class="cell-info">
                <span class="cell-name">${displayName}</span>
                ${returnTypeHtml}
                ${depsHtml}
            </div>
            <div class="cell-actions">
                <span class="cell-timing" id="timing-${cell.id}"></span>
                ${statusHtml}
                ${actionButton}
                <button class="btn btn-icon btn-insert" data-cell-id="${cell.id}" data-action="insert-cell" title="Insert cell below">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-duplicate" data-cell-id="${cell.id}" data-action="duplicate-cell" title="Duplicate cell">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-move" data-cell-id="${cell.id}" data-action="move-cell-up" title="Move cell up">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6-6 6z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-move" data-cell-id="${cell.id}" data-action="move-cell-down" title="Move cell down">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M7.41 8.59L12 13.17l4.59-4.58L18 10l-6 6-6-6z"/>
                    </svg>
                </button>
                <button class="btn btn-icon btn-delete" data-cell-id="${cell.id}" data-action="delete-cell" title="Delete cell">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z"/>
                    </svg>
                </button>
            </div>
        </div>
        ${isDefinition ? docHtml : descHtml}
        <div class="cell-editor" id="editor-${cell.id}"></div>
        ${!isDefinition ? '<div class="cell-output" id="output-' + cell.id + '" style="display: none;"></div>' : ''}
    `;

    // Create Monaco editor after element is in DOM
    setTimeout(() => {
        if (state.monacoReady && !state.editors.has(cell.id)) {
            // Always get fresh cell data from state to avoid race conditions
            const freshCell = state.cells.get(cell.id);
            if (!freshCell) return;

            const source = isDefinition ? (freshCell.content || '') : freshCell.source;
            createEditor(cell.id, source);
        }
    }, 0);

    // Show output if available (only for code cells)
    if (!isDefinition && (cell.output || cell.error || cell.compileErrors)) {
        setTimeout(() => {
            if (cell.error) {
                updateCellError(cell.id);
            } else if (cell.compileErrors) {
                updateCellCompileErrors(cell.id);
            } else if (cell.output) {
                updateCellOutput(cell.id);
            }
        }, 0);
    }

    return div;
}

function createEditor(cellId, source) {
    const container = document.getElementById(`editor-${cellId}`);
    if (!container || !state.monacoReady) return;

    // Prevent duplicate editor creation
    if (state.editors.has(cellId)) return;

    const editor = monaco.editor.create(container, {
        value: source,
        language: 'rust',
        theme: 'venus-dark',
        minimap: { enabled: false },
        lineNumbers: 'on',
        scrollBeyondLastLine: false,
        automaticLayout: true,
        fixedOverflowWidgets: true,  // Allow hover/autocomplete widgets to escape container
        fontSize: 13,
        fontFamily: "'JetBrains Mono', 'Fira Code', Consolas, monospace",
        tabSize: 4,
        insertSpaces: true,
        folding: true,
        wordWrap: 'off',
        renderLineHighlight: 'line',
        selectOnLineNumbers: true,
        roundedSelection: true,
        cursorBlinking: 'smooth',
        cursorSmoothCaretAnimation: 'on',
        smoothScrolling: true,
        padding: { top: 8, bottom: 8 },
        scrollbar: {
            vertical: 'hidden',
            horizontal: 'auto',
            verticalScrollbarSize: 0,
            horizontalScrollbarSize: 10
        }
    });

    // Auto-resize editor based on content (no max height - page scrolls instead)
    const updateHeight = () => {
        const lineCount = editor.getModel().getLineCount();
        const lineHeight = 19; // Approximate line height
        const minHeight = 80;
        const contentHeight = Math.max(minHeight, lineCount * lineHeight + 16);
        container.style.height = `${contentHeight}px`;
        editor.layout();
    };

    editor.onDidChangeModelContent(() => {
        updateHeight();
        // Mark cell as dirty and update content
        const cell = state.cells.get(cellId);
        if (cell) {
            cell.dirty = true;
            // Definition cells use 'content', code cells use 'source'
            if (cell.cell_type === 'definition') {
                cell.content = editor.getValue();
            } else {
                cell.source = editor.getValue();
            }
        }
        // Notify LSP of document change
        if (typeof notifyDocumentChange === 'function') {
            notifyDocumentChange();
        }
    });

    updateHeight();
    state.editors.set(cellId, editor);

    // Force layout recalculation after a tiny delay to fix visual issues
    setTimeout(() => {
        editor.layout();
    }, 10);
}

function getStatusHtml(status) {
    const icons = {
        idle: '',
        running: '<div class="spinner"></div>',
        compiling: '<div class="spinner"></div>',
        success: '✓',
        error: '✗'
    };

    const labels = {
        idle: 'Idle',
        running: 'Running',
        compiling: 'Compiling',
        success: 'Success',
        error: 'Error'
    };

    return `<span class="cell-status ${status}">${icons[status] || ''} ${labels[status] || status}</span>`;
}

function updateCellStatus(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell) return;

    const cellEl = document.getElementById(`cell-${cellId}`);
    if (!cellEl) return;

    // Update cell class
    cellEl.className = `cell ${cell.status}`;

    // Update status badge
    const actionsEl = cellEl.querySelector('.cell-actions');
    if (actionsEl) {
        const statusEl = actionsEl.querySelector('.cell-status');
        if (statusEl) {
            statusEl.outerHTML = getStatusHtml(cell.status);
        }
    }

    // Update cell run button - toggle between Run and Stop
    const runBtn = document.getElementById(`run-btn-${cellId}`);
    if (runBtn) {
        if (cell.status === 'running') {
            runBtn.innerHTML = `
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                    <rect x="6" y="6" width="12" height="12" rx="1"/>
                </svg>
            `;
            runBtn.onclick = interruptExecution;
            runBtn.title = 'Stop Execution';
            runBtn.classList.add('btn-danger');
        } else {
            runBtn.innerHTML = `
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M8 5v14l11-7z"/>
                </svg>
            `;
            runBtn.onclick = () => executeCell(cellId);
            runBtn.title = 'Run Cell';
            runBtn.classList.remove('btn-danger');
        }
    }

    // Graph hidden (plotr in development)
    // if (state.graphVisible && typeof updateGraphNodeStatus === 'function') {
    //     updateGraphNodeStatus(cellId, cell.status);
    // }
}

function updateCellTiming(cellId, durationMs) {
    const timingEl = document.getElementById(`timing-${cellId}`);
    if (timingEl) {
        if (durationMs < 1000) {
            timingEl.textContent = `${durationMs}ms`;
        } else {
            timingEl.textContent = `${(durationMs / 1000).toFixed(2)}s`;
        }
    }
}

// =====================================
// Widget Rendering
// =====================================

/**
 * Render widgets for a cell.
 * @param {number} cellId - The cell ID
 * @param {Array} widgets - Array of widget definitions
 * @returns {string} HTML for all widgets
 */
function renderWidgets(cellId, widgets) {
    if (!widgets || widgets.length === 0) return '';

    const widgetsHtml = widgets.map(widget => renderWidget(cellId, widget)).join('');
    return `<div class="cell-widgets">${widgetsHtml}</div>`;
}

/**
 * Render a single widget.
 * @param {number} cellId - The cell ID
 * @param {Object} widget - Widget definition
 * @returns {string} HTML for the widget
 */
function renderWidget(cellId, widget) {
    switch (widget.type) {
        case 'slider':
            return renderSliderWidget(cellId, widget);
        case 'text_input':
            return renderTextInputWidget(cellId, widget);
        case 'select':
            return renderSelectWidget(cellId, widget);
        case 'checkbox':
            return renderCheckboxWidget(cellId, widget);
        default:
            console.warn('Unknown widget type:', widget.type);
            return '';
    }
}

/**
 * Render a slider widget.
 */
function renderSliderWidget(cellId, widget) {
    return `
        <div class="widget widget-slider">
            <label class="widget-label">${escapeHtml(widget.label)}</label>
            <div class="widget-slider-container">
                <input type="range"
                    class="widget-slider-input"
                    data-cell-id="${cellId}"
                    data-widget-id="${escapeHtml(widget.id)}"
                    data-widget-type="slider"
                    min="${widget.min}"
                    max="${widget.max}"
                    step="${widget.step}"
                    value="${widget.value}">
                <span class="widget-slider-value">${widget.value}</span>
            </div>
        </div>
    `;
}

/**
 * Render a text input widget.
 */
function renderTextInputWidget(cellId, widget) {
    return `
        <div class="widget widget-text">
            <label class="widget-label">${escapeHtml(widget.label)}</label>
            <input type="text"
                class="widget-text-input"
                data-cell-id="${cellId}"
                data-widget-id="${escapeHtml(widget.id)}"
                data-widget-type="text_input"
                placeholder="${escapeHtml(widget.placeholder)}"
                value="${escapeHtml(widget.value)}">
        </div>
    `;
}

/**
 * Render a select widget.
 */
function renderSelectWidget(cellId, widget) {
    const optionsHtml = widget.options.map((opt, idx) =>
        `<option value="${idx}" ${idx === widget.selected ? 'selected' : ''}>${escapeHtml(opt)}</option>`
    ).join('');

    return `
        <div class="widget widget-select">
            <label class="widget-label">${escapeHtml(widget.label)}</label>
            <select
                class="widget-select-input"
                data-cell-id="${cellId}"
                data-widget-id="${escapeHtml(widget.id)}"
                data-widget-type="select">
                ${optionsHtml}
            </select>
        </div>
    `;
}

/**
 * Render a checkbox widget.
 */
function renderCheckboxWidget(cellId, widget) {
    return `
        <div class="widget widget-checkbox">
            <label class="widget-checkbox-label">
                <input type="checkbox"
                    class="widget-checkbox-input"
                    data-cell-id="${cellId}"
                    data-widget-id="${escapeHtml(widget.id)}"
                    data-widget-type="checkbox"
                    ${widget.value ? 'checked' : ''}>
                <span>${escapeHtml(widget.label)}</span>
            </label>
        </div>
    `;
}

/**
 * Send a widget value update to the server.
 */
function sendWidgetUpdate(cellId, widgetId, value) {
    send({
        type: 'widget_update',
        cell_id: cellId,
        widget_id: widgetId,
        value: value
    });
}

function updateCellOutput(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell || !cell.output) return;

    const outputEl = document.getElementById(`output-${cellId}`);
    if (!outputEl) return;

    const output = cell.output;
    let contentType = 'text';
    let content = '';

    if (output.html) {
        contentType = 'html';
        content = output.html;
    } else if (output.image) {
        contentType = 'image';
        content = `<img src="data:image/png;base64,${output.image}" alt="Cell output">`;
    } else if (output.json) {
        contentType = 'text';
        content = JSON.stringify(output.json, null, 2);
    } else if (output.text) {
        contentType = 'text';
        content = escapeHtml(output.text);
    }

    // Render widgets if present
    const widgetsHtml = renderWidgets(cellId, output.widgets);

    // Add re-run button for all outputs (useful to re-run without scrolling to top)
    const rerunBtn = `<button class="output-rerun-btn" onclick="executeCell(${cellId})" title="Re-run cell">${ICONS.play}</button>`;

    outputEl.innerHTML = `
        <div class="cell-output-header">
            <span>Output</span>
            <div class="output-header-controls">
                ${rerunBtn}
                ${renderHistoryControls(cellId)}
            </div>
        </div>
        ${widgetsHtml}
        <div class="cell-output-content ${contentType}">${content}</div>
    `;
    outputEl.style.display = 'block';

    // Update history controls visibility
    updateHistoryControls(cellId);
}

function updateCellError(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell || !cell.error) return;

    const outputEl = document.getElementById(`output-${cellId}`);
    if (!outputEl) return;

    const error = cell.error;
    const locationStr = error.location
        ? `<span class="error-location">Line ${error.location.line}:${error.location.column}</span>`
        : '';

    outputEl.innerHTML = `
        <div class="cell-output-header">
            <span>Error</span>
            ${renderHistoryControls(cellId)}
        </div>
        <div class="cell-error">
            ${locationStr}
            ${escapeHtml(error.message)}
        </div>
    `;
    outputEl.style.display = 'block';

    // Update history controls visibility
    updateHistoryControls(cellId);
}

function updateCellCompileErrors(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell || !cell.compileErrors) return;

    const outputEl = document.getElementById(`output-${cellId}`);
    if (!outputEl) return;

    const errorsHtml = cell.compileErrors.map(error => {
        const locationStr = error.location
            ? `<span class="error-location">Line ${error.location.line}:${error.location.column}</span>`
            : '';
        const codeStr = error.code ? `[${error.code}] ` : '';
        return `<div class="compile-error">
            ${locationStr}
            ${codeStr}${escapeHtml(error.rendered || error.message)}
        </div>`;
    }).join('');

    outputEl.innerHTML = `<div class="cell-error">${errorsHtml}</div>`;
    outputEl.style.display = 'block';
}

function updateCellCount() {
    elements.cellCount.textContent = `${state.cells.size} cell${state.cells.size !== 1 ? 's' : ''}`;
}

// =====================================
// Actions
// =====================================

function executeCell(cellId) {
    send({ type: 'execute_cell', cell_id: cellId });
}

function executeAll() {
    send({ type: 'execute_all' });
}

function interruptExecution() {
    send({ type: 'interrupt' });
}

function insertCellAfter(cellId) {
    send({ type: 'insert_cell', after_cell_id: cellId });
}

function insertCellAtEnd() {
    send({ type: 'insert_cell', after_cell_id: null });
}

function confirmDeleteCell(cellId) {
    // Find the cell name for the confirmation message
    const cell = state.cells.get(cellId);
    const cellName = cell ? cell.name : `Cell ${cellId}`;

    showConfirmDialog({
        title: 'Delete Cell',
        message: `Delete cell ${cellName}?`,
        confirmText: 'Delete',
        onConfirm: () => deleteCell(cellId)
    });
}

// Custom confirmation dialog
function showConfirmDialog({ title, message, confirmText = 'Confirm', onConfirm }) {
    // Create modal if it doesn't exist
    let overlay = document.getElementById('modal-overlay');
    if (!overlay) {
        overlay = document.createElement('div');
        overlay.id = 'modal-overlay';
        overlay.className = 'modal-overlay';
        overlay.innerHTML = `
            <div class="modal">
                <div class="modal-title"></div>
                <div class="modal-message"></div>
                <div class="modal-actions">
                    <button class="modal-btn modal-btn-cancel">Cancel</button>
                    <button class="modal-btn modal-btn-danger"></button>
                </div>
            </div>
        `;
        document.body.appendChild(overlay);

        // Close on overlay click
        overlay.addEventListener('click', (e) => {
            if (e.target === overlay) hideConfirmDialog();
        });

        // Close on Escape key
        document.addEventListener('keydown', (e) => {
            if (e.key === 'Escape' && overlay.classList.contains('visible')) {
                hideConfirmDialog();
            }
        });

        // Cancel button
        overlay.querySelector('.modal-btn-cancel').addEventListener('click', hideConfirmDialog);
    }

    // Set content
    overlay.querySelector('.modal-title').textContent = title;
    overlay.querySelector('.modal-message').textContent = message;
    const confirmBtn = overlay.querySelector('.modal-btn-danger');
    confirmBtn.textContent = confirmText;

    // Set confirm handler
    const newConfirmBtn = confirmBtn.cloneNode(true);
    confirmBtn.parentNode.replaceChild(newConfirmBtn, confirmBtn);
    newConfirmBtn.addEventListener('click', () => {
        hideConfirmDialog();
        onConfirm();
    });

    // Show modal
    overlay.classList.add('visible');
}

function hideConfirmDialog() {
    const overlay = document.getElementById('modal-overlay');
    if (overlay) {
        overlay.classList.remove('visible');
    }
}

function deleteCell(cellId) {
    send({ type: 'delete_cell', cell_id: cellId });
}

function duplicateCell(cellId) {
    send({ type: 'duplicate_cell', cell_id: cellId });
}

function moveCellUp(cellId) {
    send({ type: 'move_cell', cell_id: cellId, direction: 'up' });
}

function moveCellDown(cellId) {
    send({ type: 'move_cell', cell_id: cellId, direction: 'down' });
}

// =====================================
// Markdown Cell Operations
// =====================================

function editMarkdownCell(cellId) {
    const preview = document.getElementById(`markdown-preview-${cellId}`);
    const editor = document.getElementById(`markdown-editor-${cellId}`);

    if (preview && editor) {
        preview.style.display = 'none';
        editor.style.display = 'block';

        // Focus the textarea
        const textarea = document.getElementById(`markdown-textarea-${cellId}`);
        if (textarea) {
            textarea.focus();
        }
    }
}

function saveMarkdownCell(cellId) {
    const textarea = document.getElementById(`markdown-textarea-${cellId}`);
    if (!textarea) return;

    const newContent = textarea.value;
    send({
        type: 'edit_markdown_cell',
        cell_id: cellId,
        new_content: newContent
    });

    // Switch back to preview mode
    cancelMarkdownEdit(cellId);
}

function cancelMarkdownEdit(cellId) {
    const preview = document.getElementById(`markdown-preview-${cellId}`);
    const editor = document.getElementById(`markdown-editor-${cellId}`);
    const textarea = document.getElementById(`markdown-textarea-${cellId}`);

    if (preview && editor) {
        // Restore original content
        const cell = state.cells.get(cellId);
        if (cell && textarea) {
            textarea.value = cell.content;
        }

        preview.style.display = 'block';
        editor.style.display = 'none';
    }
}

function confirmDeleteMarkdownCell(cellId) {
    showConfirmDialog({
        title: 'Delete Markdown Cell',
        message: 'Are you sure you want to delete this markdown cell? This cannot be undone.',
        confirmText: 'Delete',
        onConfirm: () => deleteMarkdownCell(cellId)
    });
}

function deleteMarkdownCell(cellId) {
    send({ type: 'delete_markdown_cell', cell_id: cellId });
}

function moveMarkdownCellUp(cellId) {
    send({ type: 'move_markdown_cell', cell_id: cellId, direction: 'up' });
}

function moveMarkdownCellDown(cellId) {
    send({ type: 'move_markdown_cell', cell_id: cellId, direction: 'down' });
}

function insertMarkdownCellAfter(cellId) {
    // Insert a new markdown cell after the specified cell
    send({
        type: 'insert_markdown_cell',
        content: '# New Markdown Cell\n\nEdit this content...',
        after_cell_id: cellId
    });
}

function copyMarkdownCell(cellId) {
    // Get the cell content and create a duplicate
    const cell = state.cells.get(cellId);
    if (!cell || cell.cell_type !== 'markdown') return;

    // Send request to insert a copy
    send({
        type: 'insert_markdown_cell',
        content: cell.content,
        after_cell_id: cellId
    });
}

function insertMarkdownCellAtEnd() {
    // Insert a new markdown cell at the end (after the last cell)
    const lastCellId = state.sourceOrder.length > 0
        ? state.sourceOrder[state.sourceOrder.length - 1]
        : null;

    send({
        type: 'insert_markdown_cell',
        content: '# New Markdown Cell\n\nEdit this content...',
        after_cell_id: lastCellId
    });
}

// =====================================
// Definition Cell Operations
// =====================================

function saveDefinitionCell(cellId) {
    const editor = state.editors.get(cellId);
    if (!editor) return;

    const newContent = editor.getValue();
    send({
        type: 'edit_definition_cell',
        cell_id: cellId,
        new_content: newContent
    });
}

function confirmDeleteDefinitionCell(cellId) {
    showConfirmDialog({
        title: 'Delete Definition Cell',
        message: 'Are you sure you want to delete this definition cell? This cannot be undone and may break cells that depend on these definitions.',
        confirmText: 'Delete',
        onConfirm: () => deleteDefinitionCell(cellId)
    });
}

function deleteDefinitionCell(cellId) {
    send({ type: 'delete_definition_cell', cell_id: cellId });
}

function moveDefinitionCellUp(cellId) {
    send({ type: 'move_definition_cell', cell_id: cellId, direction: 'up' });
}

function moveDefinitionCellDown(cellId) {
    send({ type: 'move_definition_cell', cell_id: cellId, direction: 'down' });
}

function copyDefinitionCell(cellId) {
    // Copy definition cell content to clipboard
    const cell = state.cells.get(cellId);
    if (!cell || cell.cell_type !== 'definition') return;

    navigator.clipboard.writeText(cell.content).then(() => {
        showToast('Definition copied to clipboard', 'success');
    }).catch(err => {
        console.error('Failed to copy:', err);
        showToast('Failed to copy to clipboard', 'error');
    });
}

function undo() {
    if (state.canUndo) {
        send({ type: 'undo' });
    }
}

function redo() {
    if (state.canRedo) {
        send({ type: 'redo' });
    }
}

function syncNotebook() {
    send({ type: 'sync' });
}

function restartKernel() {
    showConfirmDialog({
        title: 'Restart Kernel',
        message: 'This will clear all execution state and outputs. Cell source code will be preserved.',
        confirmText: 'Restart',
        onConfirm: () => {
            send({ type: 'restart_kernel' });
            showToast('Restarting kernel...', 'info');
        }
    });
}

function clearOutputs() {
    showConfirmDialog({
        title: 'Clear All Outputs',
        message: 'This will clear all cell outputs. Cell source code and widget values will be preserved.',
        confirmText: 'Clear',
        onConfirm: () => {
            send({ type: 'clear_outputs' });
            showToast('Clearing outputs...', 'info');
        }
    });
}

// Graph hidden (plotr in development)
// function toggleGraph() {
//     state.graphVisible = !state.graphVisible;
//     elements.graphPanel.classList.toggle('hidden', !state.graphVisible);
//
//     if (state.graphVisible && typeof renderGraph === 'function') {
//         renderGraph(state.cells, state.executionOrder);
//     }
// }

function toggleVariableExplorer() {
    elements.variableExplorer.classList.toggle('collapsed');
    const btn = elements.explorerToggleBtn;
    const isCollapsed = elements.variableExplorer.classList.contains('collapsed');
    btn.innerHTML = isCollapsed ? ICONS.chevronRight : ICONS.chevronLeft;
    // Update header button active state
    if (elements.variablesToggleBtn) {
        elements.variablesToggleBtn.classList.toggle('active', !isCollapsed);
    }
}

function renderVariableExplorer() {
    const content = elements.explorerContent;
    if (!content) return;

    // Get cells in execution order
    const orderedCells = state.executionOrder
        .map(id => state.cells.get(id))
        .filter(cell => cell);

    if (orderedCells.length === 0) {
        content.innerHTML = '<div class="explorer-empty">No variables yet</div>';
        return;
    }

    content.innerHTML = orderedCells.map(cell => {
        const preview = getOutputPreview(cell);
        const statusClass = cell.status || 'idle';
        const statusIndicator = getVariableStatusIndicator(cell.status);

        return `
            <div class="variable-item ${statusClass}" data-cell-id="${cell.id}" data-action="scroll-to-cell">
                <div class="variable-name">${escapeHtml(cell.name)}</div>
                <div class="variable-type">${escapeHtml(cell.return_type)}</div>
                <div class="variable-preview ${preview.class}">${preview.content}</div>
                ${statusIndicator}
            </div>
        `;
    }).join('');
}

function getOutputPreview(cell) {
    if (cell.status === 'running') {
        return { content: 'Running...', class: 'empty' };
    }

    if (cell.error) {
        return { content: 'Error', class: 'empty' };
    }

    if (!cell.output) {
        return { content: 'Not executed', class: 'empty' };
    }

    const output = cell.output;
    let text = '';

    if (output.text) {
        text = output.text;
    } else if (output.json) {
        text = JSON.stringify(output.json);
    } else if (output.html) {
        // Strip HTML tags for preview
        const temp = document.createElement('div');
        temp.innerHTML = output.html;
        text = temp.textContent || temp.innerText || '';
    } else if (output.image) {
        return { content: '[Image]', class: '' };
    }

    // Truncate for preview
    const maxLen = 100;
    const truncated = text.length > maxLen ? text.substring(0, maxLen) + '...' : text;
    const isMultiline = truncated.includes('\n');

    return {
        content: escapeHtml(truncated.trim()) || 'Empty output',
        class: isMultiline ? 'multiline' : ''
    };
}

function getVariableStatusIndicator(status) {
    switch (status) {
        case 'running':
            return '<div class="variable-status"><div class="spinner"></div> Running</div>';
        case 'success':
            return '';
        case 'error':
            return '<div class="variable-status" style="color: var(--error);">Error</div>';
        default:
            return '';
    }
}

function scrollToCell(cellId) {
    const cellEl = document.getElementById(`cell-${cellId}`);
    if (cellEl) {
        cellEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
        // Brief highlight effect
        cellEl.style.boxShadow = '0 0 0 2px var(--accent-primary)';
        setTimeout(() => {
            cellEl.style.boxShadow = '';
        }, 1000);
    }
}

function updateVariableItem(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell) return;

    const item = elements.explorerContent?.querySelector(`[data-cell-id="${cellId}"]`);
    if (!item) {
        // Re-render the whole explorer if item doesn't exist
        renderVariableExplorer();
        return;
    }

    const preview = getOutputPreview(cell);
    const statusClass = cell.status || 'idle';
    const statusIndicator = getVariableStatusIndicator(cell.status);

    item.className = `variable-item ${statusClass}`;
    item.innerHTML = `
        <div class="variable-name">${escapeHtml(cell.name)}</div>
        <div class="variable-type">${escapeHtml(cell.return_type)}</div>
        <div class="variable-preview ${preview.class}">${preview.content}</div>
        ${statusIndicator}
    `;
}

// =====================================
// Execution History
// =====================================

/**
 * Estimate the size of a history entry in bytes.
 */
function estimateEntrySize(entry) {
    let size = 0;
    if (entry.output) size += JSON.stringify(entry.output).length;
    if (entry.error) size += entry.error.length;
    if (entry.source) size += entry.source.length;
    return size;
}

/**
 * Truncate large outputs to stay within memory limits.
 */
function truncateOutput(output, maxSize) {
    if (!output) return output;
    const str = typeof output === 'string' ? output : JSON.stringify(output);
    if (str.length <= maxSize) return output;

    // Truncate and add indicator
    const truncated = str.substring(0, maxSize - 50);
    return typeof output === 'string'
        ? truncated + '\n... [output truncated for memory]'
        : truncated + '... [truncated]';
}

/**
 * Calculate total history size across all cells.
 */
function getTotalHistorySize() {
    let total = 0;
    for (const history of state.executionHistory.values()) {
        for (const entry of history) {
            total += estimateEntrySize(entry);
        }
    }
    return total;
}

/**
 * Prune oldest history entries to stay within memory limits.
 */
function pruneHistoryIfNeeded() {
    while (getTotalHistorySize() > HISTORY_CONFIG.maxTotalSize) {
        // Find the cell with the oldest entry
        let oldestTime = Infinity;
        let oldestCellId = null;

        for (const [cellId, history] of state.executionHistory.entries()) {
            if (history.length > 1 && history[0].timestamp < oldestTime) {
                oldestTime = history[0].timestamp;
                oldestCellId = cellId;
            }
        }

        if (oldestCellId === null) break;

        // Remove the oldest entry
        const history = state.executionHistory.get(oldestCellId);
        history.shift();

        // Update history index if needed
        const cell = state.cells.get(oldestCellId);
        if (cell && cell.historyIndex > 0) {
            cell.historyIndex--;
        }
    }
}

/**
 * Add an execution result to history.
 */
function addToHistory(cellId, entry) {
    if (!state.executionHistory.has(cellId)) {
        state.executionHistory.set(cellId, []);
    }

    // Truncate large outputs before storing
    const truncatedEntry = {
        timestamp: Date.now(),
        output: truncateOutput(entry.output, HISTORY_CONFIG.maxOutputSize),
        error: entry.error,
        duration: entry.duration,
        source: entry.source,
    };

    const history = state.executionHistory.get(cellId);
    history.push(truncatedEntry);

    // Trim history if too long (per-cell limit)
    while (history.length > HISTORY_CONFIG.maxEntriesPerCell) {
        history.shift();
    }

    // Prune global history if total size exceeds limit
    pruneHistoryIfNeeded();

    // Update the cell's current history index
    const cell = state.cells.get(cellId);
    if (cell) {
        cell.historyIndex = history.length - 1;
    }
}

/**
 * Get history for a cell.
 */
function getCellHistory(cellId) {
    return state.executionHistory.get(cellId) || [];
}

/**
 * Navigate to a specific history entry.
 * Sends message to server to update the actual cell value.
 */
function navigateHistory(cellId, index) {
    const history = getCellHistory(cellId);
    if (index < 0 || index >= history.length) {
        return;
    }

    const cell = state.cells.get(cellId);
    if (!cell) return;

    // Update local state optimistically
    cell.historyIndex = index;
    const entry = history[index];

    // Update cell output from history
    cell.output = entry.output;
    cell.error = entry.error;

    // Update display
    if (entry.error) {
        updateCellError(cellId);
    } else if (entry.output) {
        updateCellOutput(cellId);
    }

    // Update history controls
    updateHistoryControls(cellId);

    // Notify server to update the actual value for dependent cells
    send({
        type: 'select_history',
        cell_id: cellId,
        index: index
    });
}

/**
 * Go to previous history entry.
 */
function historyPrev(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell) return;

    const currentIndex = cell.historyIndex ?? (getCellHistory(cellId).length - 1);
    if (currentIndex > 0) {
        navigateHistory(cellId, currentIndex - 1);
    }
}

/**
 * Go to next history entry.
 */
function historyNext(cellId) {
    const cell = state.cells.get(cellId);
    if (!cell) return;

    const history = getCellHistory(cellId);
    const currentIndex = cell.historyIndex ?? (history.length - 1);
    if (currentIndex < history.length - 1) {
        navigateHistory(cellId, currentIndex + 1);
    }
}

/**
 * Update history navigation controls.
 */
function updateHistoryControls(cellId) {
    const controlsEl = document.getElementById(`history-controls-${cellId}`);
    if (!controlsEl) return;

    const history = getCellHistory(cellId);
    const cell = state.cells.get(cellId);
    const currentIndex = cell?.historyIndex ?? (history.length - 1);

    if (history.length <= 1) {
        controlsEl.style.display = 'none';
        return;
    }

    controlsEl.style.display = 'flex';
    const prevBtn = controlsEl.querySelector('.history-prev');
    const nextBtn = controlsEl.querySelector('.history-next');
    const infoSpan = controlsEl.querySelector('.history-info');

    if (prevBtn) {
        prevBtn.disabled = currentIndex <= 0;
    }
    if (nextBtn) {
        nextBtn.disabled = currentIndex >= history.length - 1;
    }
    if (infoSpan) {
        const entry = history[currentIndex];
        const time = entry ? formatTimestamp(entry.timestamp) : '';
        infoSpan.textContent = `${currentIndex + 1}/${history.length} • ${time}`;
    }
}

/**
 * Format timestamp for display.
 */
function formatTimestamp(timestamp) {
    const date = new Date(timestamp);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/**
 * Render history controls for a cell.
 */
function renderHistoryControls(cellId) {
    return `
        <div class="history-controls" id="history-controls-${cellId}" style="display: none;">
            <button class="btn btn-icon history-prev" data-cell-id="${cellId}" data-action="history-prev" title="Previous execution">
                ${ICONS.chevronLeftSmall}
            </button>
            <span class="history-info">1/1</span>
            <button class="btn btn-icon history-next" data-cell-id="${cellId}" data-action="history-next" title="Next execution">
                ${ICONS.chevronRightSmall}
            </button>
        </div>
    `;
}

// =====================================
// Utilities
// =====================================

/**
 * Create a debounced version of a function.
 * @param {Function} fn - Function to debounce
 * @param {number} delay - Delay in milliseconds
 * @returns {Function} Debounced function
 */
function debounce(fn, delay) {
    let timeoutId = null;
    return function(...args) {
        if (timeoutId) {
            clearTimeout(timeoutId);
        }
        timeoutId = setTimeout(() => {
            fn.apply(this, args);
            timeoutId = null;
        }, delay);
    };
}

/**
 * Create a debounced widget update sender per widget.
 * Each widget gets its own debounced sender to avoid cross-widget interference.
 */
const widgetUpdateDebounceMap = new Map();
const WIDGET_DEBOUNCE_MS = 50;  // 50ms debounce for responsive feel

function getDebouncedWidgetSender(cellId, widgetId) {
    const key = `${cellId}:${widgetId}`;
    if (!widgetUpdateDebounceMap.has(key)) {
        widgetUpdateDebounceMap.set(key, debounce((value) => {
            sendWidgetUpdate(cellId, widgetId, value);
        }, WIDGET_DEBOUNCE_MS));
    }
    return widgetUpdateDebounceMap.get(key);
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function showToast(message, type = 'info') {
    const toast = document.createElement('div');
    toast.className = `toast ${type}`;
    toast.textContent = message;

    elements.toastContainer.appendChild(toast);

    setTimeout(() => {
        toast.style.opacity = '0';
        setTimeout(() => toast.remove(), 300);
    }, 4000);
}

// =====================================
// Event Listeners
// =====================================

elements.runAllBtn.addEventListener('click', executeAll);
elements.syncBtn.addEventListener('click', syncNotebook);
elements.restartKernelBtn.addEventListener('click', restartKernel);
elements.clearOutputsBtn.addEventListener('click', clearOutputs);
document.getElementById('undo-btn').addEventListener('click', undo);
document.getElementById('redo-btn').addEventListener('click', redo);
// Graph hidden (plotr in development)
// elements.graphToggleBtn.addEventListener('click', toggleGraph);
// elements.graphCloseBtn.addEventListener('click', toggleGraph);
elements.explorerToggleBtn.addEventListener('click', toggleVariableExplorer);
elements.variablesToggleBtn.addEventListener('click', toggleVariableExplorer);

// Event delegation for dynamically created elements
// This replaces inline onclick handlers for better maintainability and memory efficiency
document.addEventListener('click', (e) => {
    const target = e.target.closest('[data-action]');
    if (!target) return;

    const action = target.dataset.action;
    const cellId = parseInt(target.dataset.cellId, 10);

    if (isNaN(cellId) && action !== 'interrupt' && action !== 'insert-cell-end' && action !== 'insert-markdown-end') return;

    switch (action) {
        case 'run-cell':
            executeCell(cellId);
            break;
        case 'insert-cell':
            insertCellAfter(cellId);
            break;
        case 'insert-cell-end':
            insertCellAtEnd();
            break;
        case 'delete-cell':
            confirmDeleteCell(cellId);
            break;
        case 'duplicate-cell':
            duplicateCell(cellId);
            break;
        case 'move-cell-up':
            moveCellUp(cellId);
            break;
        case 'move-cell-down':
            moveCellDown(cellId);
            break;
        case 'scroll-to-cell':
            scrollToCell(cellId);
            break;
        case 'history-prev':
            historyPrev(cellId);
            break;
        case 'history-next':
            historyNext(cellId);
            break;
        case 'edit-markdown':
            editMarkdownCell(cellId);
            break;
        case 'save-markdown':
            saveMarkdownCell(cellId);
            break;
        case 'cancel-markdown':
            cancelMarkdownEdit(cellId);
            break;
        case 'delete-markdown':
            confirmDeleteMarkdownCell(cellId);
            break;
        case 'move-markdown-up':
            moveMarkdownCellUp(cellId);
            break;
        case 'move-markdown-down':
            moveMarkdownCellDown(cellId);
            break;
        case 'insert-markdown':
            insertMarkdownCellAfter(cellId);
            break;
        case 'copy-markdown':
            copyMarkdownCell(cellId);
            break;
        case 'insert-markdown-end':
            insertMarkdownCellAtEnd();
            break;
        case 'save-definition':
            saveDefinitionCell(cellId);
            break;
        case 'delete-definition':
            confirmDeleteDefinitionCell(cellId);
            break;
        case 'move-definition-up':
            moveDefinitionCellUp(cellId);
            break;
        case 'move-definition-down':
            moveDefinitionCellDown(cellId);
            break;
        case 'copy-definition':
            copyDefinitionCell(cellId);
            break;
    }
});

// Keyboard shortcuts
document.addEventListener('keydown', (e) => {
    // Shift+Enter: Execute all
    if (e.shiftKey && e.key === 'Enter') {
        e.preventDefault();
        executeAll();
        return;
    }

    // Ctrl+Z: Undo (but not inside Monaco editor)
    if ((e.ctrlKey || e.metaKey) && e.key === 'z' && !e.shiftKey) {
        // Only handle if not focused in an editor
        const activeEditor = document.activeElement?.closest('.cell-editor');
        if (!activeEditor) {
            e.preventDefault();
            undo();
            return;
        }
    }

    // Ctrl+Shift+Z or Ctrl+Y: Redo (but not inside Monaco editor)
    if ((e.ctrlKey || e.metaKey) && ((e.key === 'z' && e.shiftKey) || e.key === 'y')) {
        // Only handle if not focused in an editor
        const activeEditor = document.activeElement?.closest('.cell-editor');
        if (!activeEditor) {
            e.preventDefault();
            redo();
            return;
        }
    }
});

// Widget event delegation for sliders (real-time feedback with debouncing)
document.addEventListener('input', (e) => {
    const target = e.target;
    if (!target.dataset.widgetType) return;

    const cellId = parseInt(target.dataset.cellId, 10);
    const widgetId = target.dataset.widgetId;
    const widgetType = target.dataset.widgetType;

    if (isNaN(cellId)) return;

    if (widgetType === 'slider') {
        // Update the displayed value immediately (UI responsiveness)
        const valueDisplay = target.nextElementSibling;
        if (valueDisplay) {
            valueDisplay.textContent = target.value;
        }
        // Send update to server with debouncing to prevent flooding
        const debouncedSend = getDebouncedWidgetSender(cellId, widgetId);
        debouncedSend(parseFloat(target.value));
    } else if (widgetType === 'text_input') {
        // Debounce text input for as-you-type updates
        const debouncedSend = getDebouncedWidgetSender(cellId, widgetId);
        debouncedSend(target.value);
    }
});

// Widget event delegation for change events (text, select, checkbox)
document.addEventListener('change', (e) => {
    const target = e.target;
    if (!target.dataset.widgetType) return;

    const cellId = parseInt(target.dataset.cellId, 10);
    const widgetId = target.dataset.widgetId;
    const widgetType = target.dataset.widgetType;

    if (isNaN(cellId)) return;

    let value;
    switch (widgetType) {
        case 'text_input':
            value = target.value;
            break;
        case 'select':
            value = parseInt(target.value, 10);
            break;
        case 'checkbox':
            value = target.checked;
            break;
        default:
            return;
    }

    sendWidgetUpdate(cellId, widgetId, value);
});

// Expose only essential functions globally (for debugging/console access)
window.interruptExecution = interruptExecution;

// =====================================
// Initialize
// =====================================

// Set initial active state for Variables toggle button (panel is open by default)
if (elements.variablesToggleBtn && !elements.variableExplorer.classList.contains('collapsed')) {
    elements.variablesToggleBtn.classList.add('active');
}

connect();
