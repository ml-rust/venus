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
        console.log('LSP WebSocket connected');
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
    switch (method) {
        case 'textDocument/publishDiagnostics':
            handleDiagnostics(params);
            break;
        case 'window/showMessage':
            console.log('LSP message:', params.message);
            break;
        case 'window/logMessage':
            console.debug('LSP log:', params.message);
            break;
        default:
            console.debug('LSP notification:', method, params);
    }
}

/**
 * Initialize LSP connection.
 */
async function initializeLsp() {
    try {
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
                workspace: {
                    workspaceFolders: true,
                },
            },
            rootUri: null,
            workspaceFolders: null,
        });

        lspState.capabilities = result.capabilities;
        lspState.initialized = true;

        // Send initialized notification
        sendLspNotification('initialized', {});

        console.log('LSP initialized with capabilities:', result.capabilities);

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

    // Get combined source from all cells
    const content = getCombinedSource();

    sendLspNotification('textDocument/didOpen', {
        textDocument: {
            uri: `file://${state.notebookPath}`,
            languageId: 'rust',
            version: ++lspState.documentVersion,
            text: content,
        },
    });
}

/**
 * Get combined source code from all cells.
 */
function getCombinedSource() {
    const lines = [];

    // Add module-level items first (dependencies declaration, etc.)
    // For now, we combine cell sources

    state.executionOrder.forEach(cellId => {
        const cell = state.cells.get(cellId);
        if (cell && cell.source) {
            lines.push(cell.source);
            lines.push('');  // Empty line between cells
        }
    });

    return lines.join('\n');
}

/**
 * Notify LSP of document change.
 */
function notifyDocumentChange() {
    if (!lspState.initialized) {
        return;
    }

    const content = getCombinedSource();

    sendLspNotification('textDocument/didChange', {
        textDocument: {
            uri: `file://${state.notebookPath}`,
            version: ++lspState.documentVersion,
        },
        contentChanges: [
            { text: content },
        ],
    });
}

/**
 * Handle diagnostics from LSP.
 */
function handleDiagnostics(params) {
    // Map diagnostics to Monaco markers
    // For now, log them
    if (params.diagnostics && params.diagnostics.length > 0) {
        console.log('LSP diagnostics:', params.diagnostics);
    }
}

/**
 * Request completions at a position.
 */
async function requestCompletions(cellId, line, character) {
    if (!lspState.initialized) {
        return [];
    }

    // Calculate global position from cell position
    const globalPos = cellToGlobalPosition(cellId, line, character);
    if (!globalPos) {
        return [];
    }

    try {
        const result = await sendLspRequest('textDocument/completion', {
            textDocument: {
                uri: `file://${state.notebookPath}`,
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
    if (!lspState.initialized) {
        return null;
    }

    const globalPos = cellToGlobalPosition(cellId, line, character);
    if (!globalPos) {
        return null;
    }

    try {
        const result = await sendLspRequest('textDocument/hover', {
            textDocument: {
                uri: `file://${state.notebookPath}`,
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
 */
function cellToGlobalPosition(cellId, line, character) {
    let globalLine = 0;

    for (const id of state.executionOrder) {
        if (id === cellId) {
            return {
                line: globalLine + line,
                character,
            };
        }

        const cell = state.cells.get(id);
        if (cell && cell.source) {
            // Count lines in this cell + 1 empty line
            globalLine += cell.source.split('\n').length + 1;
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
