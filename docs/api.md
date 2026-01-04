# Venus Server API Reference

The Venus server exposes a WebSocket-based API for real-time notebook interaction, plus REST endpoints for querying state. This API allows building custom frontends, IDE plugins, or programmatic notebook control.

## Overview

**Architecture**: Venus provides a built-in web frontend for ease of use, but the server API is designed for custom frontends. Advanced users can build their own UIs using the documented protocol below.

**Endpoints**:

- `ws://localhost:8080/ws` - WebSocket for notebook operations
- `ws://localhost:8080/lsp` - WebSocket for LSP (rust-analyzer) integration
- `GET /health` - Health check
- `GET /api/state` - Current notebook state
- `GET /api/graph` - Dependency graph

**Protocol**: JSON messages over WebSocket. All messages are tagged with a `type` field for discrimination.

## ⚠️ Security Notice

**The Venus server API executes arbitrary Rust code received over WebSocket with NO sandboxing.**

**For custom frontend developers:**

- Deploy Venus server in **isolated environment** (container/VM)
- **Never expose publicly** without authentication and isolation
- Treat all notebook code as **potentially malicious**
- **Provider responsibility**: YOU must secure the deployment - Venus provides no security

Venus cells have full system access:

- Can read/write/delete any file
- Can make network requests
- Can spawn processes
- Can execute any system calls

**This is equivalent to exposing `cargo run` over the internet.** See [SECURITY.md](../SECURITY.md) for the complete security policy.

## Getting Started

### 1. Connect to WebSocket

```javascript
const ws = new WebSocket("ws://localhost:8080/ws");

ws.onopen = () => {
  console.log("Connected to Venus server");
  // Server automatically sends initial NotebookState on connection
};

ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  console.log("Server message:", message);
};
```

### 2. Send Commands

```javascript
// Execute a cell
ws.send(
  JSON.stringify({
    type: "execute_cell",
    cell_id: 1,
  })
);

// Get current state
ws.send(
  JSON.stringify({
    type: "get_state",
  })
);
```

### 3. Receive Updates

The server broadcasts state changes to all connected clients:

```javascript
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);

  switch (msg.type) {
    case "notebook_state":
      updateNotebookUI(msg.cells, msg.execution_order);
      break;
    case "cell_completed":
      displayCellOutput(msg.cell_id, msg.output);
      break;
    case "cell_error":
      showError(msg.cell_id, msg.error);
      break;
  }
};
```

## REST API

### GET /health

Health check endpoint.

**Response**:

```json
{
  "status": "ok",
  "version": "0.1.0"
}
```

### GET /api/state

Get current notebook state.

**Response**: Returns a `NotebookState` message (see [Server Messages](#server-messages)).

### GET /api/graph

Get dependency graph information.

**Response**:

```json
{
  "execution_order": [1, 2, 3, 4]
}
```

## WebSocket API

### Client Messages

Messages sent from client to server. All messages must include a `type` field.

#### Notebook Querying

**GetState**

```json
{ "type": "get_state" }
```

Request the complete notebook state.

**GetGraph**

```json
{ "type": "get_graph" }
```

Request the dependency graph.

#### Cell Execution

**ExecuteCell**

```json
{
  "type": "execute_cell",
  "cell_id": 1
}
```

Execute a specific cell.

**ExecuteAll**

```json
{ "type": "execute_all" }
```

Execute all cells in dependency order.

**ExecuteDirty**

```json
{ "type": "execute_dirty" }
```

Execute only cells marked as dirty (needing re-execution).

**Interrupt**

```json
{ "type": "interrupt" }
```

Abort currently running execution.

#### Cell Management

**InsertCell**

```json
{
  "type": "insert_cell",
  "after_cell_id": 2 // Optional: null for end
}
```

Insert a new code cell.

**DeleteCell**

```json
{
  "type": "delete_cell",
  "cell_id": 3
}
```

Delete a cell.

**DuplicateCell**

```json
{
  "type": "duplicate_cell",
  "cell_id": 1
}
```

Duplicate an existing cell.

**MoveCell**

```json
{
  "type": "move_cell",
  "cell_id": 2,
  "direction": "up" // "up" or "down"
}
```

Move a cell up or down in source order.

**RenameCell**

```json
{
  "type": "rename_cell",
  "cell_id": 1,
  "new_display_name": "My Cell"
}
```

Rename a cell's display name.

**CellEdit**

```json
{
  "type": "cell_edit",
  "cell_id": 1,
  "source": "pub fn new_code() -> String { ... }"
}
```

Edit a cell's source code (marks cell as dirty).

#### Markdown Cells

**InsertMarkdownCell**

```json
{
  "type": "insert_markdown_cell",
  "content": "# My Markdown\n\nSome content",
  "after_cell_id": null // Optional
}
```

**EditMarkdownCell**

```json
{
  "type": "edit_markdown_cell",
  "cell_id": 5,
  "new_content": "Updated markdown content"
}
```

**DeleteMarkdownCell**

```json
{
  "type": "delete_markdown_cell",
  "cell_id": 5
}
```

**MoveMarkdownCell**

```json
{
  "type": "move_markdown_cell",
  "cell_id": 5,
  "direction": "down"
}
```

#### Definition Cells

Definition cells contain types, imports, and helper functions compiled into the universe.

**InsertDefinitionCell**

```json
{
  "type": "insert_definition_cell",
  "content": "struct MyStruct { field: i32 }",
  "definition_type": "struct", // "struct", "enum", "trait", "use", "impl", "const", "static", "type_alias", "fn"
  "after_cell_id": null
}
```

**EditDefinitionCell**

```json
{
  "type": "edit_definition_cell",
  "cell_id": 10,
  "new_content": "struct UpdatedStruct { field: String }"
}
```

**DeleteDefinitionCell**

```json
{
  "type": "delete_definition_cell",
  "cell_id": 10
}
```

**MoveDefinitionCell**

```json
{
  "type": "move_definition_cell",
  "cell_id": 10,
  "direction": "up"
}
```

#### Interactive Widgets

**WidgetUpdate**

```json
{
  "type": "widget_update",
  "cell_id": 2,
  "widget_id": "my_slider",
  "value": {
    "Float": 0.75 // Or: {"Int": 42}, {"String": "hello"}, {"Bool": true}
  }
}
```

Update a widget value. Does NOT trigger re-execution automatically.

#### Output History

**SelectHistory**

```json
{
  "type": "select_history",
  "cell_id": 1,
  "index": 2 // 0 = oldest
}
```

Select a previous output from cell history (for cells that have been executed multiple times).

#### Undo/Redo

**Undo**

```json
{ "type": "undo" }
```

Undo the last cell management operation.

**Redo**

```json
{ "type": "redo" }
```

Redo the last undone operation.

#### Kernel Management

**RestartKernel**

```json
{ "type": "restart_kernel" }
```

Kill worker process pool, clear memory state, preserve source code.

**ClearOutputs**

```json
{ "type": "clear_outputs" }
```

Clear all cell outputs without restarting.

#### Notebook Export

**Sync**

```json
{ "type": "sync" }
```

Export notebook to `.ipynb` format for GitHub preview.

### Server Messages

Messages sent from server to clients. Server automatically broadcasts state changes to all connected clients.

#### Notebook State

**NotebookState**

```json
{
  "type": "notebook_state",
  "path": "/path/to/notebook.rs",
  "cells": [
    {
      "cell_type": "code",
      "id": 1,
      "name": "my_cell",
      "display_name": "My Cell",
      "source": "pub fn my_cell() -> String { ... }",
      "description": "Doc comment",
      "return_type": "String",
      "dependencies": ["other_cell"],
      "status": "idle", // "idle", "running", "completed", "error"
      "output": {
        /* CellOutput */
      },
      "dirty": false
    },
    {
      "cell_type": "markdown",
      "id": 2,
      "content": "# My Markdown"
    },
    {
      "cell_type": "definition",
      "id": 3,
      "content": "struct MyStruct { ... }",
      "definition_type": "struct",
      "doc_comment": "/// Documentation"
    }
  ],
  "source_order": [1, 2, 3],
  "execution_order": [1, 3],
  "workspace_root": "/path/to/workspace",
  "cargo_toml_path": "/path/to/Cargo.toml"
}
```

#### Execution Status

**CellStarted**

```json
{
  "type": "cell_started",
  "cell_id": 1
}
```

**CellCompleted**

```json
{
  "type": "cell_completed",
  "cell_id": 1,
  "duration_ms": 123,
  "output": {
    "display": "Result: 42",
    "widgets": [
      {
        "id": "slider_1",
        "widget_type": "slider",
        "label": "Value",
        "min": 0.0,
        "max": 1.0,
        "step": 0.01,
        "value": { "Float": 0.5 }
      }
    ]
  }
}
```

**CellError**

```json
{
  "type": "cell_error",
  "cell_id": 1,
  "error": "Division by zero",
  "location": {
    "file": "notebook.rs",
    "line": 15,
    "column": 20,
    "snippet": "let x = 1 / 0;"
  }
}
```

**CompileError**

```json
{
  "type": "compile_error",
  "cell_id": 1,
  "errors": [
    {
      "message": "mismatched types",
      "severity": "error",
      "code": "E0308",
      "line": 10,
      "column": 5,
      "snippet": "expected `String`, found `i32`"
    }
  ]
}
```

**ExecutionAborted**

```json
{
  "type": "execution_aborted",
  "cell_id": 1 // Optional
}
```

#### Graph Updates

**GraphUpdated**

```json
{
  "type": "graph_updated",
  "edges": [
    { "from": 2, "to": 1 },
    { "from": 3, "to": 1 }
  ],
  "levels": [[2, 3], [1]] // Parallel execution levels
}
```

#### File Watching

**FileChanged**

```json
{
  "type": "file_changed",
  "modified_cells": [1, 2],
  "added_cells": [
    {
      /* CellState */
    }
  ],
  "removed_cells": [5]
}
```

#### Operation Results

**CellInserted** / **CellDeleted** / **CellDuplicated** / **CellMoved** / **CellRenamed**

```json
{
  "type": "cell_inserted",
  "cell_id": 10,
  "error": null // or "Error message" if failed
}
```

**MarkdownCellInserted** / **MarkdownCellEdited** / **MarkdownCellDeleted** / **MarkdownCellMoved**

```json
{
  "type": "markdown_cell_inserted",
  "cell_id": 5,
  "error": null
}
```

**DefinitionCellInserted** / **DefinitionCellEdited** / **DefinitionCellDeleted** / **DefinitionCellMoved**

```json
{
  "type": "definition_cell_edited",
  "cell_id": 3,
  "error": null,
  "dirty_cells": [1, 2] // Cells affected by definition change
}
```

**UndoResult** / **RedoResult**

```json
{
  "type": "undo_result",
  "success": true,
  "error": null,
  "description": "Deleted cell 'my_cell'"
}
```

**UndoRedoState**

```json
{
  "type": "undo_redo_state",
  "can_undo": true,
  "can_redo": false,
  "undo_description": "Delete cell 'foo'",
  "redo_description": null
}
```

**HistorySelected**

```json
{
  "type": "history_selected",
  "cell_id": 1,
  "index": 2,
  "count": 5,
  "output": {
    /* CellOutput */
  },
  "dirty_cells": [2, 3] // Cells now needing re-execution
}
```

**KernelRestarted**

```json
{
  "type": "kernel_restarted",
  "error": null
}
```

**OutputsCleared**

```json
{
  "type": "outputs_cleared",
  "error": null
}
```

**SyncCompleted**

```json
{
  "type": "sync_completed",
  "ipynb_path": "/path/to/notebook.ipynb"
}
```

**Error**

```json
{
  "type": "error",
  "message": "Operation failed: ..."
}
```

## Type Definitions

### CellId

Integer identifier for cells. Unique within a notebook.

### CellStatus

Enum: `"idle"`, `"running"`, `"completed"`, `"error"`

### MoveDirection

Enum: `"up"`, `"down"`

### DefinitionType

Enum: `"struct"`, `"enum"`, `"trait"`, `"use"`, `"impl"`, `"const"`, `"static"`, `"type_alias"`, `"fn"`

### WidgetValue

Tagged union:

- `{"Int": 42}`
- `{"Float": 3.14}`
- `{"String": "hello"}`
- `{"Bool": true}`

### CellOutput

```typescript
{
  display: string,       // Formatted output for display
  widgets?: WidgetDef[]  // Interactive widget definitions
}
```

### WidgetDef

```typescript
{
  id: string,
  widget_type: "slider" | "text" | "dropdown" | "checkbox",
  label: string,

  // For slider:
  min?: number,
  max?: number,
  step?: number,

  // For dropdown:
  options?: string[],

  value: WidgetValue
}
```

## Building a Custom Frontend

### Minimal Example

```html
<!DOCTYPE html>
<html>
  <head>
    <title>Venus Client</title>
  </head>
  <body>
    <h1>Custom Venus Frontend</h1>
    <div id="cells"></div>
    <button onclick="executeAll()">Execute All</button>

    <script>
      const ws = new WebSocket("ws://localhost:8080/ws");
      let cells = [];

      ws.onmessage = (e) => {
        const msg = JSON.parse(e.data);

        if (msg.type === "notebook_state") {
          cells = msg.cells;
          renderCells();
        } else if (msg.type === "cell_completed") {
          updateCellOutput(msg.cell_id, msg.output);
        }
      };

      function renderCells() {
        const container = document.getElementById("cells");
        container.innerHTML = cells
          .map((cell) => {
            if (cell.cell_type === "code") {
              return `
            <div class="cell">
              <h3>${cell.display_name}</h3>
              <pre>${cell.source}</pre>
              <button onclick="executeCell(${cell.id})">Execute</button>
              <div id="output-${cell.id}"></div>
            </div>
          `;
            }
            return "";
          })
          .join("");
      }

      function executeCell(cellId) {
        ws.send(JSON.stringify({ type: "execute_cell", cell_id: cellId }));
      }

      function executeAll() {
        ws.send(JSON.stringify({ type: "execute_all" }));
      }

      function updateCellOutput(cellId, output) {
        const div = document.getElementById(`output-${cellId}`);
        if (div && output) {
          div.textContent = output.display;
        }
      }
    </script>
  </body>
</html>
```

### Best Practices

1. **Subscribe to all server messages**: The server broadcasts state changes to all clients
2. **Handle reconnection**: WebSocket may disconnect; implement reconnect logic
3. **Track undo/redo state**: Use `UndoRedoState` to enable/disable UI buttons
4. **Show execution status**: Display cell status (`running`, `completed`, `error`)
5. **Handle errors gracefully**: All operations return `error` field on failure
6. **Respect dirty flags**: UI should indicate which cells need re-execution

## API Stability

**Current Status**: Active Development (0.1.x)

The WebSocket protocol is stabilizing but may change before 1.0. Breaking changes will be noted in release notes.

**Future Plans**:

- Additional REST endpoints for stateless operations
- Batch execution API
- Authentication/authorization
- Multi-notebook support

## See Also

- [CLI Reference](cli.md) - Command-line interface
- [Getting Started](getting-started.md) - Notebook basics
- [Widgets](widgets.md) - Interactive widget usage
