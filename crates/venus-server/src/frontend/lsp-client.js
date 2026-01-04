/**
 * LSP Client for Monaco-rust-analyzer integration.
 *
 * Provides language intelligence features via rust-analyzer.
 */

// LSP Client state
const lspState = {
    ws: null,
    connected: false,
    initialized: false,
    pendingRequests: new Map(),
    requestId: 0,
    capabilities: null,
    documentVersion: 0,
};

/**
 * Connect to the LSP server.
 */
function connectLsp() {
    if (lspState.ws && lspState.connected) {
        return;
    }

    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${location.host}/lsp`;

    lspState.ws = new WebSocket(wsUrl);

    lspState.ws.onopen = () => {
        lspState.connected = true;
        console.log('[LSP] WebSocket connected');
        initializeLsp();
    };

    lspState.ws.onmessage = (event) => {
        try {
            const msg = JSON.parse(event.data);
            handleLspMessage(msg);
        } catch (e) {
            console.error('Failed to parse LSP message:', e);
        }
    };

    lspState.ws.onclose = () => {
        lspState.connected = false;
        lspState.initialized = false;
        console.log('LSP WebSocket disconnected');
        // Try to reconnect after delay
        setTimeout(connectLsp, 5000);
    };

    lspState.ws.onerror = (error) => {
        console.error('LSP WebSocket error:', error);
        if (typeof showToast === 'function') {
            showToast('Code intelligence disconnected. Reconnecting...', 'warning');
        }
    };
}

/**
 * Send LSP request and return promise for response.
 */
function sendLspRequest(method, params) {
    return new Promise((resolve, reject) => {
        if (!lspState.connected) {
            reject(new Error('LSP not connected'));
            return;
        }

        const id = ++lspState.requestId;
        const request = {
            jsonrpc: '2.0',
            id,
            method,
            params,
        };

        lspState.pendingRequests.set(id, { resolve, reject });
        lspState.ws.send(JSON.stringify(request));

        // Timeout after 10 seconds
        setTimeout(() => {
            if (lspState.pendingRequests.has(id)) {
                lspState.pendingRequests.delete(id);
                reject(new Error('LSP request timeout'));
            }
        }, 10000);
    });
}

/**
 * Send LSP notification (no response expected).
 */
function sendLspNotification(method, params) {
    if (!lspState.connected) {
        return;
    }

    const notification = {
        jsonrpc: '2.0',
        method,
        params,
    };

    lspState.ws.send(JSON.stringify(notification));
}

/**
 * Handle incoming LSP message.
 */
function handleLspMessage(msg) {
    // Response to a request
    if (msg.id !== undefined && lspState.pendingRequests.has(msg.id)) {
        const { resolve, reject } = lspState.pendingRequests.get(msg.id);
        lspState.pendingRequests.delete(msg.id);

        if (msg.error) {
            reject(new Error(msg.error.message));
        } else {
            resolve(msg.result);
        }
        return;
    }

    // Notification from server
    if (msg.method) {
        handleLspNotification(msg.method, msg.params);
    }
}

/**
 * Handle LSP notification.
 */
function handleLspNotification(method, params) {
    // Log all notifications to see what rust-analyzer is sending
    console.log('[LSP] Notification:', method, params?.uri || '');

    switch (method) {
        case 'textDocument/publishDiagnostics':
            handleDiagnostics(params);
            break;
        case 'window/showMessage':
            handleLspShowMessage(params);
            break;
        case 'window/logMessage':
            console.debug('LSP log:', params.message);
            break;
        default:
            console.debug('LSP notification:', method, params);
    }
}

/**
 * Handle LSP window/showMessage notification.
 * Shows user-friendly messages for rust-analyzer issues.
 */
function handleLspShowMessage(params) {
    const message = params.message || '';
    const type = params.type; // 1=Error, 2=Warning, 3=Info, 4=Log

    // Map LSP message types to toast types
    let toastType = 'info';
    if (type === 1) toastType = 'error';
    else if (type === 2) toastType = 'warning';

    // Make error messages more user-friendly
    let displayMessage = message;

    if (message.includes('proc-macro server')) {
        displayMessage = 'rust-analyzer: Proc-macro expansion limited (version mismatch). Code analysis still works.';
        toastType = 'warning';
    } else if (message.includes('Failed to spawn')) {
        displayMessage = 'rust-analyzer failed to start. Some code intelligence features may be unavailable.';
        toastType = 'error';
    } else if (message.includes('Failed to run')) {
        displayMessage = 'rust-analyzer encountered an error. Try reloading the page.';
        toastType = 'error';
    }

    // Only show important messages to avoid spam
    if (type <= 2 && typeof showToast === 'function') {
        showToast(displayMessage, toastType);
    }

    console.log('LSP message:', message);
}

/**
 * Get notebook directory from path (browser-compatible).
 */
function getNotebookDir() {
    if (!state.notebookPath) return '';
    return state.notebookPath.substring(0, state.notebookPath.lastIndexOf('/'));
}

/**
 * Initialize LSP connection.
 */
async function initializeLsp() {
    try {
        // Use the universe package Cargo.toml for proper dependency resolution
        const notebookDir = getNotebookDir();
        const universeCargoToml = `${notebookDir}/.venus/build/universe/Cargo.toml`;

        console.log('[LSP] Notebook dir:', notebookDir);
        console.log('[LSP] Universe Cargo.toml:', universeCargoToml);
        console.log('[LSP] Using universe package for LSP analysis');

        const result = await sendLspRequest('initialize', {
            processId: null,
            clientInfo: {
                name: 'Venus Notebook',
                version: '1.0.0',
            },
            capabilities: {
                textDocument: {
                    completion: {
                        completionItem: {
                            snippetSupport: true,
                            documentationFormat: ['markdown', 'plaintext'],
                        },
                    },
                    hover: {
                        contentFormat: ['markdown', 'plaintext'],
                    },
                    signatureHelp: {
                        signatureInformation: {
                            documentationFormat: ['markdown', 'plaintext'],
                        },
                    },
                    publishDiagnostics: {
                        relatedInformation: true,
                    },
                },
            },
            // Set the universe directory as the workspace root
            // This makes rust-analyzer treat it as a standalone workspace
            rootUri: `file://${notebookDir}/.venus/build/universe`,
            workspaceFolders: null,
            initializationOptions: {
                'rust-analyzer': {
                    // Disable check on save to reduce noise
                    checkOnSave: false,
                },
            },
        });

        lspState.capabilities = result.capabilities;
        lspState.initialized = true;

        // Send initialized notification
        sendLspNotification('initialized', {});

        console.log('[LSP] Initialized with capabilities:', result.capabilities);

        // Open the notebook document
        openNotebookDocument();
    } catch (e) {
        console.error('LSP initialization failed:', e);
    }
}

/**
 * Open the notebook document in LSP.
 */
function openNotebookDocument() {
    if (!state.notebookPath) {
        return;
    }

    // Use the universe package's virtual file location
    // This is a proper Cargo package that rust-analyzer will analyze
    const notebookDir = getNotebookDir();
    const virtualUri = `file://${notebookDir}/.venus/build/universe/src/notebook.rs`;
    const content = getCombinedSource();

    console.log('[LSP] Opening virtual document:', virtualUri);
    console.log('[LSP] Virtual document has', content.split('\n').length, 'lines');

    sendLspNotification('textDocument/didOpen', {
        textDocument: {
            uri: virtualUri,
            languageId: 'rust',
            version: ++lspState.documentVersion,
            text: content,
        },
    });
}

/**
 * Get combined source code from all cells.
 * Uses sourceOrder to match the actual .rs file structure.
 * Includes definition cells and code cells, but not markdown cells.
 */
function getCombinedSource() {
    const lines = [];

    // Iterate in source file order (includes both definition and code cells)
    state.sourceOrder.forEach(cellId => {
        const cell = state.cells.get(cellId);
        if (!cell) {
            return;
        }

        // Skip markdown cells - they don't need LSP
        if (cell.cell_type === 'markdown') {
            return;
        }

        // Definition cells have 'content', code cells have 'source'
        const cellContent = cell.cell_type === 'definition' ? cell.content : cell.source;

        if (cellContent) {
            lines.push(cellContent);
            lines.push('');  // Empty line between cells
        }
    });

    return lines.join('\n');
}

/**
 * Notify LSP of document change.
 */
function notifyDocumentChange() {
    if (!lspState.initialized || !state.notebookPath) {
        return;
    }

    const notebookDir = getNotebookDir();
    const virtualUri = `file://${notebookDir}/.venus/build/universe/src/notebook.rs`;
    const content = getCombinedSource();

    console.log('[LSP] Notifying document change with', content.split('\n').length, 'lines');

    sendLspNotification('textDocument/didChange', {
        textDocument: {
            uri: virtualUri,
            version: ++lspState.documentVersion,
        },
        contentChanges: [
            { text: content },
        ],
    });
}

/**
 * Handle diagnostics from LSP.
 * Maps LSP diagnostics to Monaco markers for each cell.
 */
function handleDiagnostics(params) {
    console.log('[LSP] Received diagnostics for URI:', params.uri);
    console.log('[LSP] Diagnostic count:', params.diagnostics.length);

    // ONLY process diagnostics for the virtual document
    // Ignore diagnostics from other workspace files
    if (!state.notebookPath) {
        return;
    }
    const notebookDir = getNotebookDir();
    const virtualUri = `file://${notebookDir}/.venus/build/universe/src/notebook.rs`;
    if (params.uri !== virtualUri) {
        console.log('[LSP] Ignoring diagnostics for non-notebook file:', params.uri);
        return;
    }

    if (!params.diagnostics || typeof monaco === 'undefined') {
        return;
    }

    // Log first few diagnostic positions for debugging
    if (params.diagnostics.length > 0) {
        console.log('[LSP] First diagnostic at line:', params.diagnostics[0].range.start.line,
                    'message:', params.diagnostics[0].message.substring(0, 50));
    }

    // Group diagnostics by cell
    const diagnosticsByCell = new Map();

    for (const diagnostic of params.diagnostics) {
        // Convert global position to cell position
        const cellInfo = globalToCellPosition(
            diagnostic.range.start.line,
            diagnostic.range.start.character
        );

        if (!cellInfo) {
            continue;
        }

        const { cellId, line: startLine, character: startChar } = cellInfo;

        // Also convert end position
        const endInfo = globalToCellPosition(
            diagnostic.range.end.line,
            diagnostic.range.end.character
        );

        // If end is in different cell, clamp to end of start cell
        let endLine, endChar;
        if (endInfo && endInfo.cellId === cellId) {
            endLine = endInfo.line;
            endChar = endInfo.character;
        } else {
            // Clamp to end of current line
            endLine = startLine;
            endChar = startChar + 10; // Approximate
        }

        if (!diagnosticsByCell.has(cellId)) {
            diagnosticsByCell.set(cellId, []);
        }

        diagnosticsByCell.get(cellId).push({
            severity: mapDiagnosticSeverity(diagnostic.severity),
            message: diagnostic.message,
            startLineNumber: startLine + 1,  // Monaco is 1-based
            startColumn: startChar + 1,
            endLineNumber: endLine + 1,
            endColumn: endChar + 1,
            source: diagnostic.source || 'rust-analyzer',
            code: diagnostic.code,
        });
    }

    // Apply markers to each cell's editor
    for (const [cellId, markers] of diagnosticsByCell) {
        console.log(`[LSP] Setting ${markers.length} markers for cell ${cellId}`);
        const editor = state.editors.get(cellId);
        if (editor) {
            const model = editor.getModel();
            if (model) {
                console.log(`[LSP] Model found for cell ${cellId}, applying markers`);
                monaco.editor.setModelMarkers(model, 'rust-analyzer', markers);
                // Force Monaco to render the decorations
                editor.render(true);
            } else {
                console.warn(`[LSP] No model found for cell ${cellId}`);
            }
        } else {
            console.warn(`[LSP] No editor found for cell ${cellId}`);
        }
    }

    // Clear markers for cells with no diagnostics
    for (const [cellId, editor] of state.editors) {
        if (!diagnosticsByCell.has(cellId)) {
            const model = editor.getModel();
            if (model) {
                monaco.editor.setModelMarkers(model, 'rust-analyzer', []);
            }
        }
    }

    // Log summary
    const total = params.diagnostics.length;
    if (total > 0) {
        console.log(`LSP: ${total} diagnostic(s) across ${diagnosticsByCell.size} cell(s)`);
    }
}

/**
 * Convert global document position to cell-local position.
 * Returns { cellId, line, character } or null if not found.
 * Uses sourceOrder to match getCombinedSource().
 */
function globalToCellPosition(globalLine, character) {
    let currentLine = 0;

    for (const cellId of state.sourceOrder) {
        const cell = state.cells.get(cellId);
        if (!cell || cell.cell_type === 'markdown') {
            continue;
        }

        const cellContent = cell.cell_type === 'definition' ? cell.content : cell.source;
        if (!cellContent) {
            continue;
        }

        const cellLines = cellContent.split('\n').length;

        if (globalLine >= currentLine && globalLine < currentLine + cellLines) {
            return {
                cellId,
                line: globalLine - currentLine,
                character,
            };
        }

        currentLine += cellLines + 1; // +1 for empty line separator
    }

    return null;
}

/**
 * Map LSP diagnostic severity to Monaco severity.
 */
function mapDiagnosticSeverity(severity) {
    switch (severity) {
        case 1: return monaco.MarkerSeverity.Error;
        case 2: return monaco.MarkerSeverity.Warning;
        case 3: return monaco.MarkerSeverity.Info;
        case 4: return monaco.MarkerSeverity.Hint;
        default: return monaco.MarkerSeverity.Info;
    }
}

/**
 * Request completions at a position.
 */
async function requestCompletions(cellId, line, character) {
    if (!lspState.initialized || !state.notebookPath) {
        return [];
    }

    // Calculate global position from cell position
    const globalPos = cellToGlobalPosition(cellId, line, character);
    if (!globalPos) {
        return [];
    }

    try {
        const notebookDir = getNotebookDir();
        const virtualUri = `file://${notebookDir}/.venus/build/universe/src/notebook.rs`;
        const result = await sendLspRequest('textDocument/completion', {
            textDocument: {
                uri: virtualUri,
            },
            position: {
                line: globalPos.line,
                character: globalPos.character,
            },
        });

        if (!result) {
            return [];
        }

        // Handle both CompletionList and CompletionItem[] responses
        const items = result.items || result;
        return items.map(item => ({
            label: item.label,
            kind: item.kind,
            detail: item.detail,
            documentation: item.documentation,
            insertText: item.insertText || item.label,
            insertTextRules: item.insertTextFormat === 2
                ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
                : undefined,
        }));
    } catch (e) {
        console.error('Completion request failed:', e);
        return [];
    }
}

/**
 * Request hover info at a position.
 */
async function requestHover(cellId, line, character) {
    if (!lspState.initialized || !state.notebookPath) {
        return null;
    }

    const globalPos = cellToGlobalPosition(cellId, line, character);
    if (!globalPos) {
        return null;
    }

    try {
        const notebookDir = getNotebookDir();
        const virtualUri = `file://${notebookDir}/.venus/build/universe/src/notebook.rs`;
        const result = await sendLspRequest('textDocument/hover', {
            textDocument: {
                uri: virtualUri,
            },
            position: {
                line: globalPos.line,
                character: globalPos.character,
            },
        });

        if (!result || !result.contents) {
            return null;
        }

        // Convert LSP hover to Monaco format
        let contents = result.contents;
        if (typeof contents === 'string') {
            contents = [{ value: contents }];
        } else if (contents.value) {
            contents = [contents];
        }

        return {
            contents: contents.map(c => ({
                value: typeof c === 'string' ? c : c.value,
            })),
        };
    } catch (e) {
        console.error('Hover request failed:', e);
        return null;
    }
}

/**
 * Convert cell-local position to global document position.
 * Uses sourceOrder to match getCombinedSource().
 */
function cellToGlobalPosition(cellId, line, character) {
    let currentLine = 0;

    for (const id of state.sourceOrder) {
        const cell = state.cells.get(id);
        if (!cell || cell.cell_type === 'markdown') {
            continue;
        }

        if (id === cellId) {
            return {
                line: currentLine + line,
                character,
            };
        }

        const cellContent = cell.cell_type === 'definition' ? cell.content : cell.source;
        if (cellContent) {
            currentLine += cellContent.split('\n').length + 1; // +1 for empty line
        }
    }

    return null;
}

/**
 * Register Monaco language features.
 */
function registerMonacoLanguageFeatures() {
    if (typeof monaco === 'undefined') {
        // Monaco not loaded yet, try again later
        setTimeout(registerMonacoLanguageFeatures, 100);
        return;
    }

    // Register completion provider
    monaco.languages.registerCompletionItemProvider('rust', {
        triggerCharacters: ['.', ':', '<'],
        provideCompletionItems: async (model, position) => {
            // Find which cell this model belongs to
            const cellId = getCellIdFromModel(model);
            if (cellId === null) {
                return { suggestions: [] };
            }

            const suggestions = await requestCompletions(
                cellId,
                position.lineNumber - 1,  // LSP uses 0-based lines
                position.column - 1        // LSP uses 0-based columns
            );

            return {
                suggestions: suggestions.map((s, i) => ({
                    label: s.label,
                    kind: mapCompletionKind(s.kind),
                    detail: s.detail || '',
                    documentation: s.documentation,
                    insertText: s.insertText,
                    insertTextRules: s.insertTextRules,
                    sortText: String(i).padStart(5, '0'),
                })),
            };
        },
    });

    // Register hover provider
    monaco.languages.registerHoverProvider('rust', {
        provideHover: async (model, position) => {
            const cellId = getCellIdFromModel(model);
            if (cellId === null) {
                return null;
            }

            const hover = await requestHover(
                cellId,
                position.lineNumber - 1,
                position.column - 1
            );

            if (!hover) {
                return null;
            }

            return {
                contents: hover.contents.map(c => ({
                    value: c.value,
                })),
            };
        },
    });

    console.log('Monaco LSP features registered');
}

/**
 * Get cell ID from Monaco model.
 */
function getCellIdFromModel(model) {
    // Find editor that uses this model
    for (const [cellId, editor] of state.editors) {
        if (editor.getModel() === model) {
            return cellId;
        }
    }
    return null;
}

/**
 * Map LSP completion kind to Monaco completion kind.
 */
function mapCompletionKind(kind) {
    // LSP CompletionItemKind to Monaco CompletionItemKind
    const mapping = {
        1: monaco.languages.CompletionItemKind.Text,
        2: monaco.languages.CompletionItemKind.Method,
        3: monaco.languages.CompletionItemKind.Function,
        4: monaco.languages.CompletionItemKind.Constructor,
        5: monaco.languages.CompletionItemKind.Field,
        6: monaco.languages.CompletionItemKind.Variable,
        7: monaco.languages.CompletionItemKind.Class,
        8: monaco.languages.CompletionItemKind.Interface,
        9: monaco.languages.CompletionItemKind.Module,
        10: monaco.languages.CompletionItemKind.Property,
        11: monaco.languages.CompletionItemKind.Unit,
        12: monaco.languages.CompletionItemKind.Value,
        13: monaco.languages.CompletionItemKind.Enum,
        14: monaco.languages.CompletionItemKind.Keyword,
        15: monaco.languages.CompletionItemKind.Snippet,
        16: monaco.languages.CompletionItemKind.Color,
        17: monaco.languages.CompletionItemKind.File,
        18: monaco.languages.CompletionItemKind.Reference,
        19: monaco.languages.CompletionItemKind.Folder,
        20: monaco.languages.CompletionItemKind.EnumMember,
        21: monaco.languages.CompletionItemKind.Constant,
        22: monaco.languages.CompletionItemKind.Struct,
        23: monaco.languages.CompletionItemKind.Event,
        24: monaco.languages.CompletionItemKind.Operator,
        25: monaco.languages.CompletionItemKind.TypeParameter,
    };

    return mapping[kind] || monaco.languages.CompletionItemKind.Text;
}

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        registerMonacoLanguageFeatures();
        // Delay LSP connection to allow main app to initialize
        setTimeout(connectLsp, 1000);
    });
} else {
    registerMonacoLanguageFeatures();
    setTimeout(connectLsp, 1000);
}
