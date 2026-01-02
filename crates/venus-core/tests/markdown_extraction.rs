//! Test markdown cell extraction from notebooks.

use std::path::Path;
use venus_core::graph::CellParser;

#[test]
fn test_simple_notebook_markdown_extraction() {
    let path = Path::new("../../examples/simple.rs");

    let mut parser = CellParser::new();
    let result = parser.parse_file(path).expect("Failed to parse simple.rs");

    // Print code cells
    println!("\n=== CODE CELLS ===");
    println!("Found {} code cells:\n", result.code_cells.len());

    for cell in &result.code_cells {
        println!("Cell: {} (display: '{}')", cell.name, cell.display_name);
        if let Some(doc) = &cell.doc_comment {
            let preview = doc.lines().next().unwrap_or("");
            println!("  Doc: {}", preview);
        }
        println!("  Span: L{}-L{}", cell.span.start_line, cell.span.end_line);
        println!();
    }

    // Print markdown cells
    println!("\n=== MARKDOWN CELLS ===");
    println!("Found {} markdown cells:\n", result.markdown_cells.len());

    for (i, md) in result.markdown_cells.iter().enumerate() {
        println!("Markdown Cell #{}:", i);
        println!("  ID: {:?}", md.id);
        println!("  Span: L{}-L{}", md.span.start_line, md.span.end_line);
        println!("  Is module doc: {}", md.is_module_doc);
        println!("  Content preview:");
        for (j, line) in md.content.lines().take(3).enumerate() {
            println!("    {}: {}", j + 1, line);
        }
        if md.content.lines().count() > 3 {
            println!("    ... ({} more lines)", md.content.lines().count() - 3);
        }
        println!();
    }

    // Assertions
    assert_eq!(result.code_cells.len(), 4, "Should have 4 code cells");

    // Check display names are extracted from headings
    assert_eq!(result.code_cells[0].name, "config");
    assert_eq!(result.code_cells[0].display_name, "Configuration");

    assert_eq!(result.code_cells[1].name, "numbers");
    assert_eq!(result.code_cells[1].display_name, "Numbers");

    assert_eq!(result.code_cells[2].name, "sum");
    assert_eq!(result.code_cells[2].display_name, "Sum");

    assert_eq!(result.code_cells[3].name, "report");
    assert_eq!(result.code_cells[3].display_name, "Report");

    // Check markdown cells
    assert_eq!(result.markdown_cells.len(), 1, "Should have 1 markdown cell (module-level doc)");

    let module_doc = &result.markdown_cells[0];
    assert!(module_doc.is_module_doc, "Should be marked as module doc");
    assert!(module_doc.content.contains("Simple Venus Notebook"), "Should contain title");
    assert!(module_doc.content.contains("minimal notebook"), "Should contain description");

    println!("\n✓ All assertions passed!");
}
