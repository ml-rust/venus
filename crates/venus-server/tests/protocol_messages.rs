//! Integration tests for protocol message serialization.
//!
//! Tests all client and server message types for correct JSON serialization.

use venus_core::graph::CellId;
use venus_server::protocol::*;

#[test]
fn test_all_client_messages_serialize() {
    // Test all ClientMessage variants
    let messages = vec![
        ClientMessage::GetState,
        ClientMessage::CellEdit {
            cell_id: CellId::new(1),
            source: "// test code".to_string(),
        },
        ClientMessage::ExecuteCell {
            cell_id: CellId::new(1),
        },
        ClientMessage::ExecuteAll,
        ClientMessage::ExecuteDirty,
        ClientMessage::Interrupt,
        ClientMessage::Sync,
        ClientMessage::GetGraph,
        ClientMessage::WidgetUpdate {
            cell_id: CellId::new(1),
            widget_id: "slider1".to_string(),
            value: venus::widgets::WidgetValue::Number(42.0),
        },
        ClientMessage::SelectHistory {
            cell_id: CellId::new(1),
            index: 0,
        },
        ClientMessage::InsertCell {
            after_cell_id: Some(CellId::new(1)),
        },
        ClientMessage::DeleteCell {
            cell_id: CellId::new(1),
        },
        ClientMessage::DuplicateCell {
            cell_id: CellId::new(1),
        },
        ClientMessage::MoveCell {
            cell_id: CellId::new(1),
            direction: MoveDirection::Up,
        },
        ClientMessage::Undo,
        ClientMessage::Redo,
        ClientMessage::RestartKernel,
        ClientMessage::ClearOutputs,
        ClientMessage::RenameCell {
            cell_id: CellId::new(1),
            new_display_name: "New Name".to_string(),
        },
        ClientMessage::InsertMarkdownCell {
            content: "# Markdown content".to_string(),
            after_cell_id: None,
        },
        ClientMessage::EditMarkdownCell {
            cell_id: CellId::new(1),
            new_content: "Updated markdown".to_string(),
        },
        ClientMessage::DeleteMarkdownCell {
            cell_id: CellId::new(1),
        },
        ClientMessage::MoveMarkdownCell {
            cell_id: CellId::new(1),
            direction: MoveDirection::Down,
        },
    ];

    // Serialize and deserialize each message
    for msg in messages {
        let json = serde_json::to_string(&msg).expect("Failed to serialize");
        let parsed: ClientMessage =
            serde_json::from_str(&json).expect("Failed to deserialize");

        // Check that the type field matches
        let msg_type = match &msg {
            ClientMessage::GetState => "get_state",
            ClientMessage::CellEdit { .. } => "cell_edit",
            ClientMessage::ExecuteCell { .. } => "execute_cell",
            ClientMessage::ExecuteAll => "execute_all",
            ClientMessage::ExecuteDirty => "execute_dirty",
            ClientMessage::Interrupt => "interrupt",
            ClientMessage::Sync => "sync",
            ClientMessage::GetGraph => "get_graph",
            ClientMessage::WidgetUpdate { .. } => "widget_update",
            ClientMessage::SelectHistory { .. } => "select_history",
            ClientMessage::InsertCell { .. } => "insert_cell",
            ClientMessage::DeleteCell { .. } => "delete_cell",
            ClientMessage::DuplicateCell { .. } => "duplicate_cell",
            ClientMessage::MoveCell { .. } => "move_cell",
            ClientMessage::Undo => "undo",
            ClientMessage::Redo => "redo",
            ClientMessage::RestartKernel => "restart_kernel",
            ClientMessage::ClearOutputs => "clear_outputs",
            ClientMessage::RenameCell { .. } => "rename_cell",
            ClientMessage::InsertMarkdownCell { .. } => "insert_markdown_cell",
            ClientMessage::EditMarkdownCell { .. } => "edit_markdown_cell",
            ClientMessage::DeleteMarkdownCell { .. } => "delete_markdown_cell",
            ClientMessage::MoveMarkdownCell { .. } => "move_markdown_cell",
        };

        assert!(
            json.contains(msg_type),
            "Message type '{}' not found in JSON: {}",
            msg_type,
            json
        );

        // Verify roundtrip
        assert_eq!(
            std::mem::discriminant(&msg),
            std::mem::discriminant(&parsed),
            "Message variant mismatch for {}",
            msg_type
        );
    }
}

#[test]
fn test_all_server_messages_serialize() {
    // Test all ServerMessage variants
    let messages = vec![
        ServerMessage::NotebookState {
            path: "/test/notebook.rs".to_string(),
            cells: vec![],
            source_order: vec![],
            execution_order: vec![],
        },
        ServerMessage::CellStarted {
            cell_id: CellId::new(1),
        },
        ServerMessage::CellCompleted {
            cell_id: CellId::new(1),
            duration_ms: 100,
            output: None,
        },
        ServerMessage::CellError {
            cell_id: CellId::new(1),
            error: "Test error".to_string(),
            location: None,
        },
        ServerMessage::CompileError {
            cell_id: CellId::new(1),
            errors: vec![],
        },
        ServerMessage::GraphUpdated {
            edges: vec![],
            levels: vec![],
        },
        ServerMessage::FileChanged {
            modified_cells: vec![],
            added_cells: vec![],
            removed_cells: vec![],
        },
        ServerMessage::SyncCompleted {
            ipynb_path: "/test/notebook.ipynb".to_string(),
        },
        ServerMessage::ExecutionAborted {
            cell_id: Some(CellId::new(1)),
        },
        ServerMessage::Error {
            message: "Test error".to_string(),
        },
        ServerMessage::CellInserted {
            cell_id: CellId::new(2),
            error: None,
        },
        ServerMessage::CellDeleted {
            cell_id: CellId::new(1),
            error: None,
        },
        ServerMessage::CellDuplicated {
            original_cell_id: CellId::new(1),
            new_cell_id: CellId::new(2),
            error: None,
        },
        ServerMessage::CellMoved {
            cell_id: CellId::new(1),
            error: None,
        },
        ServerMessage::HistorySelected {
            cell_id: CellId::new(1),
            index: 0,
            count: 5,
            output: None,
            dirty_cells: vec![],
        },
        ServerMessage::UndoResult {
            success: true,
            error: None,
            description: Some("Deleted cell 'test'".to_string()),
        },
        ServerMessage::RedoResult {
            success: true,
            error: None,
            description: Some("Inserted cell 'test'".to_string()),
        },
        ServerMessage::UndoRedoState {
            can_undo: true,
            can_redo: false,
            undo_description: Some("Delete cell 'foo'".to_string()),
            redo_description: None,
        },
        ServerMessage::KernelRestarted { error: None },
        ServerMessage::OutputsCleared { error: None },
        ServerMessage::CellRenamed {
            cell_id: CellId::new(1),
            new_display_name: "New Name".to_string(),
            error: None,
        },
        ServerMessage::MarkdownCellInserted {
            cell_id: CellId::new(10),
            error: None,
        },
        ServerMessage::MarkdownCellEdited {
            cell_id: CellId::new(10),
            error: None,
        },
        ServerMessage::MarkdownCellDeleted {
            cell_id: CellId::new(10),
            error: None,
        },
        ServerMessage::MarkdownCellMoved {
            cell_id: CellId::new(10),
            error: None,
        },
    ];

    // Serialize and deserialize each message
    for msg in messages {
        let json = serde_json::to_string(&msg).expect("Failed to serialize");
        let parsed: ServerMessage =
            serde_json::from_str(&json).expect("Failed to deserialize");

        // Verify roundtrip (check discriminant matches)
        assert_eq!(
            std::mem::discriminant(&msg),
            std::mem::discriminant(&parsed),
            "Message variant mismatch"
        );
    }
}

#[test]
fn test_cell_state_variants() {
    // Test Code cell state
    let code_cell = CellState::Code {
        id: CellId::new(1),
        name: "test_cell".to_string(),
        display_name: "Test Cell".to_string(),
        source: "pub fn test() -> i32 { 42 }".to_string(),
        description: Some("A test function".to_string()),
        return_type: "i32".to_string(),
        dependencies: vec!["dep1".to_string(), "dep2".to_string()],
        status: CellStatus::Success,
        output: Some(CellOutput {
            text: Some("42".to_string()),
            html: None,
            image: None,
            json: None,
            widgets: vec![],
        }),
        dirty: false,
    };

    let json = serde_json::to_string(&code_cell).unwrap();
    let parsed: CellState = serde_json::from_str(&json).unwrap();

    assert_eq!(code_cell.id(), parsed.id());
    assert_eq!(code_cell.name(), parsed.name());
    assert_eq!(code_cell.is_dirty(), parsed.is_dirty());
    assert_eq!(code_cell.status(), parsed.status());

    // Test Markdown cell state
    let md_cell = CellState::Markdown {
        id: CellId::new(2),
        content: "# Markdown Title\n\nSome content".to_string(),
    };

    let json = serde_json::to_string(&md_cell).unwrap();
    let parsed: CellState = serde_json::from_str(&json).unwrap();

    assert_eq!(md_cell.id(), parsed.id());
    assert!(md_cell.name().is_none());
    assert!(!md_cell.is_dirty()); // Markdown cells can't be dirty
}

#[test]
fn test_cell_status_serialization() {
    let statuses = vec![
        CellStatus::Idle,
        CellStatus::Compiling,
        CellStatus::Running,
        CellStatus::Success,
        CellStatus::Error,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let parsed: CellStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, parsed);
    }

    // Test default
    assert_eq!(CellStatus::default(), CellStatus::Idle);
}

#[test]
fn test_cell_output_with_widgets() {
    use venus::widgets::WidgetDef;

    let output = CellOutput {
        text: Some("Result: 42".to_string()),
        html: Some("<p>Result: 42</p>".to_string()),
        image: Some("data:image/png;base64,iVBORw0KGg".to_string()),
        json: Some(serde_json::json!({"value": 42})),
        widgets: vec![
            WidgetDef::Slider {
                id: "slider1".to_string(),
                label: "Value".to_string(),
                min: 0.0,
                max: 100.0,
                step: 1.0,
                value: 50.0,
            },
            WidgetDef::Checkbox {
                id: "checkbox1".to_string(),
                label: "Enable".to_string(),
                value: true,
            },
        ],
    };

    let json = serde_json::to_string(&output).unwrap();
    let parsed: CellOutput = serde_json::from_str(&json).unwrap();

    assert_eq!(output.text, parsed.text);
    assert_eq!(output.html, parsed.html);
    assert_eq!(output.image, parsed.image);
    assert_eq!(output.widgets.len(), parsed.widgets.len());
}

#[test]
fn test_source_location_serialization() {
    let loc = SourceLocation {
        line: 10,
        column: 5,
        end_line: Some(12),
        end_column: Some(20),
    };

    let json = serde_json::to_string(&loc).unwrap();
    let parsed: SourceLocation = serde_json::from_str(&json).unwrap();

    assert_eq!(loc.line, parsed.line);
    assert_eq!(loc.column, parsed.column);
    assert_eq!(loc.end_line, parsed.end_line);
    assert_eq!(loc.end_column, parsed.end_column);
}

#[test]
fn test_compile_error_info() {
    let error = CompileErrorInfo {
        message: "expected `;`".to_string(),
        code: Some("E0308".to_string()),
        location: Some(SourceLocation {
            line: 5,
            column: 10,
            end_line: None,
            end_column: None,
        }),
        rendered: Some("error: expected `;`\n --> file.rs:5:10".to_string()),
    };

    let json = serde_json::to_string(&error).unwrap();
    let parsed: CompileErrorInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(error.message, parsed.message);
    assert_eq!(error.code, parsed.code);
    assert!(parsed.location.is_some());
    assert!(parsed.rendered.is_some());
}

#[test]
fn test_dependency_edge() {
    let edge = DependencyEdge {
        from: CellId::new(1),
        to: CellId::new(2),
        param_name: "input_data".to_string(),
    };

    let json = serde_json::to_string(&edge).unwrap();
    let parsed: DependencyEdge = serde_json::from_str(&json).unwrap();

    assert_eq!(edge.from, parsed.from);
    assert_eq!(edge.to, parsed.to);
    assert_eq!(edge.param_name, parsed.param_name);
}

#[test]
fn test_cell_state_methods() {
    let mut cell = CellState::Code {
        id: CellId::new(1),
        name: "test".to_string(),
        display_name: "Test".to_string(),
        source: "".to_string(),
        description: None,
        return_type: "()".to_string(),
        dependencies: vec![],
        status: CellStatus::Idle,
        output: None,
        dirty: false,
    };

    // Test dirty flag
    assert!(!cell.is_dirty());
    cell.set_dirty(true);
    assert!(cell.is_dirty());

    // Test status
    assert_eq!(cell.status(), Some(CellStatus::Idle));
    cell.set_status(CellStatus::Running);
    assert_eq!(cell.status(), Some(CellStatus::Running));

    // Test output
    cell.set_output(Some(CellOutput {
        text: Some("result".to_string()),
        html: None,
        image: None,
        json: None,
        widgets: vec![],
    }));
    assert!(matches!(
        &cell,
        CellState::Code {
            output: Some(_),
            ..
        }
    ));

    cell.clear_output();
    assert!(matches!(
        &cell,
        CellState::Code { output: None, .. }
    ));

    // Test markdown cell (should not have dirty/status)
    let md_cell = CellState::Markdown {
        id: CellId::new(2),
        content: "".to_string(),
    };

    assert!(!md_cell.is_dirty());
    assert!(md_cell.status().is_none());
    assert!(md_cell.name().is_none());
}
