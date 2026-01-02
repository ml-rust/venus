//! Source file editor for inserting, deleting, and reordering cells in .rs notebook files.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::{Attribute, File};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Direction for moving a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveDirection {
    /// Move cell up (swap with previous cell).
    Up,
    /// Move cell down (swap with next cell).
    Down,
}

/// Editor for modifying .rs notebook source files.
pub struct SourceEditor {
    /// Path to the source file.
    path: PathBuf,
    /// Current file content.
    content: String,
}

impl SourceEditor {
    /// Load a source file for editing.
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;

        Ok(Self {
            path: path.to_path_buf(),
            content,
        })
    }

    /// Insert a new cell after the specified cell.
    ///
    /// If `after_cell_id` is None, inserts at the end of the file.
    /// Returns the name of the newly created cell.
    pub fn insert_cell(&mut self, after_cell_id: Option<&str>) -> Result<String> {
        // Parse the file to find cell positions and existing names
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        // Collect existing cell names for unique name generation
        let existing_names = self.collect_cell_names(&file);

        // Generate a unique name for the new cell
        let new_name = self.generate_unique_name(&existing_names);

        // Find the position to insert the new cell
        let insert_pos = self.find_insert_position(&file, after_cell_id)?;

        // Generate the cell code
        let cell_code = self.generate_cell_code(&new_name);

        // Insert the cell code at the position
        self.content.insert_str(insert_pos, &cell_code);

        Ok(new_name)
    }

    /// Delete a cell by name.
    ///
    /// Returns the name of the deleted cell.
    pub fn delete_cell(&mut self, cell_name: &str) -> Result<String> {
        // Parse the file to find cell positions
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        // Find the cell's span (including doc comments and attributes)
        let (start_line, end_line) = self.find_cell_span(&file, cell_name)?;

        // Convert line numbers to byte offsets
        let lines: Vec<&str> = self.content.lines().collect();
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);

        // Remove the cell from content
        self.content = format!(
            "{}{}",
            &self.content[..start_offset],
            &self.content[end_offset..]
        );

        // Clean up extra blank lines
        self.cleanup_blank_lines();

        Ok(cell_name.to_string())
    }

    /// Duplicate a cell by name.
    ///
    /// Creates a copy of the cell with a unique name (e.g., `cell_name_copy`).
    /// The new cell is inserted immediately after the original.
    /// Returns the name of the new cell.
    pub fn duplicate_cell(&mut self, cell_name: &str) -> Result<String> {
        // Parse the file to find cell positions and existing names
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        // Find the cell's span
        let (start_line, end_line) = self.find_cell_span(&file, cell_name)?;

        // Collect existing names to generate unique copy name
        let existing_names = self.collect_cell_names(&file);

        // Generate a unique name for the copy
        let new_name = self.generate_copy_name(cell_name, &existing_names);

        // Extract the cell's source code
        let lines: Vec<&str> = self.content.lines().collect();
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);
        let cell_source = &self.content[start_offset..end_offset];

        // Replace the function name in the duplicated code
        let new_cell_source = cell_source.replace(
            &format!("fn {}(", cell_name),
            &format!("fn {}(", new_name),
        );

        // Insert the new cell after the original
        let insert_code = format!("\n{}", new_cell_source);
        self.content.insert_str(end_offset, &insert_code);

        Ok(new_name)
    }

    /// Move a cell up or down by swapping with its neighbor.
    ///
    /// Returns Ok(()) on success.
    pub fn move_cell(&mut self, cell_name: &str, direction: MoveDirection) -> Result<()> {
        // Parse the file to find all cells in order
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        // Collect all cells with their spans in order
        let cells = self.collect_cell_spans(&file);

        // Find the target cell's index
        let cell_idx = cells
            .iter()
            .position(|(name, _, _)| name == cell_name)
            .ok_or_else(|| Error::CellNotFound(format!("Cell '{}' not found", cell_name)))?;

        // Find the neighbor to swap with
        let neighbor_idx = match direction {
            MoveDirection::Up => {
                if cell_idx == 0 {
                    return Err(Error::InvalidOperation("Cannot move first cell up".to_string()));
                }
                cell_idx - 1
            }
            MoveDirection::Down => {
                if cell_idx >= cells.len() - 1 {
                    return Err(Error::InvalidOperation("Cannot move last cell down".to_string()));
                }
                cell_idx + 1
            }
        };

        // Get spans for both cells (ensure first is before second)
        let (first_idx, second_idx) = if cell_idx < neighbor_idx {
            (cell_idx, neighbor_idx)
        } else {
            (neighbor_idx, cell_idx)
        };

        let (_, first_start, first_end) = cells[first_idx];
        let (_, second_start, second_end) = cells[second_idx];

        // Extract source code for both cells
        let lines: Vec<&str> = self.content.lines().collect();
        let first_start_offset = self.line_start_offset(first_start, &lines);
        let first_end_offset = self.line_to_byte_offset(first_end, &lines);
        let second_start_offset = self.line_start_offset(second_start, &lines);
        let second_end_offset = self.line_to_byte_offset(second_end, &lines);

        let first_source = self.content[first_start_offset..first_end_offset].to_string();
        let second_source = self.content[second_start_offset..second_end_offset].to_string();

        // Build new content by replacing both cells in reverse order (to preserve offsets)
        let mut new_content = String::new();
        new_content.push_str(&self.content[..first_start_offset]);
        new_content.push_str(&second_source);
        new_content.push_str(&self.content[first_end_offset..second_start_offset]);
        new_content.push_str(&first_source);
        new_content.push_str(&self.content[second_end_offset..]);

        self.content = new_content;

        Ok(())
    }

    /// Rename a cell's display name by updating its doc comment.
    ///
    /// Updates or adds a `# Display Name` heading to the cell's doc comment.
    pub fn rename_cell(&mut self, cell_name: &str, new_display_name: &str) -> Result<()> {
        // Parse the file to find the cell
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        // Find the cell
        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                if Self::has_cell_attribute(&func.attrs) {
                    let name = func.sig.ident.to_string();
                    if name == cell_name {
                        // Extract existing doc comments (excluding # heading lines)
                        let mut doc_lines: Vec<String> = Vec::new();

                        for attr in &func.attrs {
                            if attr.path().is_ident("doc")
                                && let syn::Meta::NameValue(nv) = &attr.meta
                                && let syn::Expr::Lit(syn::ExprLit {
                                    lit: syn::Lit::Str(s),
                                    ..
                                }) = &nv.value
                            {
                                let line = s.value();
                                let trimmed = line.trim_start();

                                // Skip existing # heading (we'll add new one)
                                if trimmed.starts_with('#') {
                                    continue;
                                }

                                doc_lines.push(line);
                            }
                        }

                        // Build new doc comment with display name heading
                        let mut new_doc_lines = vec![format!("# {}", new_display_name)];
                        if !doc_lines.is_empty() {
                            // Add blank line between heading and description
                            new_doc_lines.push(String::new());
                            new_doc_lines.extend(doc_lines);
                        }

                        // Find the span for doc comments and attributes
                        let doc_start_line = if !func.attrs.is_empty() {
                            func.attrs
                                .iter()
                                .filter(|a| a.path().is_ident("doc"))
                                .map(|a| a.span().start().line)
                                .min()
                                .unwrap_or(func.attrs[0].span().start().line)
                        } else {
                            func.span().start().line
                        };

                        // Find the function declaration line (pub fn ...)
                        let fn_start_line = func.sig.fn_token.span.start().line;

                        // Reconstruct the cell with new doc comments
                        let lines: Vec<&str> = self.content.lines().collect();

                        // Get the indentation of the original doc comments or function
                        let indent = if !func.attrs.is_empty() {
                            Self::get_line_indent(&lines, doc_start_line)
                        } else {
                            Self::get_line_indent(&lines, fn_start_line)
                        };

                        // Build new doc comment block
                        let new_doc_comment = new_doc_lines
                            .iter()
                            .map(|line| format!("{}/// {}", indent, line))
                            .collect::<Vec<_>>()
                            .join("\n");

                        // Find where to replace
                        let replace_start = self.line_start_offset(doc_start_line, &lines);
                        let replace_end = self.line_start_offset(fn_start_line, &lines);

                        // Build new content
                        let mut new_content = String::new();
                        new_content.push_str(&self.content[..replace_start]);
                        new_content.push_str(&new_doc_comment);
                        new_content.push('\n');

                        // Add the #[venus::cell] attribute if it's not a doc comment
                        let mut added_cell_attr = false;
                        for attr in &func.attrs {
                            if !attr.path().is_ident("doc") {
                                if !added_cell_attr {
                                    new_content.push_str(&format!("{}#[venus::cell]\n", indent));
                                    added_cell_attr = true;
                                }
                            }
                        }

                        if !added_cell_attr {
                            new_content.push_str(&format!("{}#[venus::cell]\n", indent));
                        }

                        new_content.push_str(&self.content[replace_end..]);

                        self.content = new_content;
                        return Ok(());
                    }
                }
            }
        }

        Err(Error::CellNotFound(format!("Cell '{}' not found", cell_name)))
    }

    /// Insert a markdown cell at a specific line position.
    ///
    /// If `after_line` is None, inserts at the beginning of the file.
    /// Content should be plain markdown text (without `///` prefix).
    pub fn insert_markdown_cell(&mut self, content: &str, after_line: Option<usize>) -> Result<()> {
        let lines: Vec<&str> = self.content.lines().collect();

        // Format content as regular comment block (//)
        let markdown_block = content
            .lines()
            .map(|line| format!("// {}", line))
            .collect::<Vec<_>>()
            .join("\n");

        // Determine insertion point
        let insert_offset = if let Some(line_num) = after_line {
            if line_num > lines.len() {
                self.content.len()
            } else {
                // Find the end of the function/block at this line
                // We need to skip past the entire function body to insert after it
                self.find_block_end(line_num, &lines)
            }
        } else {
            0 // Insert at beginning
        };

        // Insert markdown block with appropriate spacing
        let insert_text = if insert_offset == 0 {
            format!("{}\n\n", markdown_block)
        } else {
            format!("\n\n{}\n", markdown_block)
        };

        self.content.insert_str(insert_offset, &insert_text);

        Ok(())
    }

    /// Find the byte offset after the closing brace of a block starting at the given line.
    fn find_block_end(&self, start_line: usize, lines: &[&str]) -> usize {
        if start_line == 0 || start_line > lines.len() {
            return self.content.len();
        }

        let mut brace_depth = 0;
        let mut found_opening = false;
        let mut offset = 0;

        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;

            // Calculate offset for this line
            if line_num < start_line {
                offset += line.len() + 1; // +1 for newline
                continue;
            }

            // Count braces
            for ch in line.chars() {
                offset += ch.len_utf8();
                match ch {
                    '{' => {
                        brace_depth += 1;
                        found_opening = true;
                    }
                    '}' => {
                        brace_depth -= 1;
                        // If we're back to 0 and we found an opening brace, we're done
                        if found_opening && brace_depth == 0 {
                            offset += 1; // Include the newline after closing brace
                            return offset.min(self.content.len());
                        }
                    }
                    _ => {}
                }
            }

            offset += 1; // newline
        }

        // If we didn't find a complete block, return end of content
        self.content.len()
    }

    /// Edit an existing markdown cell by line range.
    ///
    /// Replaces the comment block at the given line range with new content.
    /// If `is_module_doc` is true, uses `//!` syntax; otherwise uses `///`.
    pub fn edit_markdown_cell(&mut self, start_line: usize, end_line: usize, new_content: &str, is_module_doc: bool) -> Result<()> {
        let lines: Vec<&str> = self.content.lines().collect();

        if start_line == 0 || start_line > lines.len() || end_line > lines.len() || start_line > end_line {
            return Err(Error::InvalidOperation(format!(
                "Invalid line range: {}-{}",
                start_line, end_line
            )));
        }

        // Format new content as comment block (either //! or ///)
        let comment_prefix = if is_module_doc { "//!" } else { "///" };
        let markdown_block = new_content
            .lines()
            .map(|line| format!("{} {}", comment_prefix, line))
            .collect::<Vec<_>>()
            .join("\n");

        // Calculate byte offsets
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);

        // Replace the old block with new content
        // Note: end_offset already points past the newline of end_line
        // and markdown_block has internal newlines but no trailing newline
        let needs_newline = end_offset < self.content.len();
        self.content = if needs_newline {
            format!(
                "{}{}\n{}",
                &self.content[..start_offset],
                markdown_block,
                &self.content[end_offset..]
            )
        } else {
            // Last line of file - no trailing newline needed
            format!(
                "{}{}",
                &self.content[..start_offset],
                markdown_block
            )
        };

        eprintln!("  needs_newline={}", needs_newline);

        Ok(())
    }

    /// Delete a markdown cell by line range.
    pub fn delete_markdown_cell(&mut self, start_line: usize, end_line: usize) -> Result<()> {
        let lines: Vec<&str> = self.content.lines().collect();

        if start_line == 0 || start_line > lines.len() || end_line > lines.len() || start_line > end_line {
            return Err(Error::InvalidOperation(format!(
                "Invalid line range: {}-{}",
                start_line, end_line
            )));
        }

        // Calculate byte offsets
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);

        // Remove the markdown block
        self.content = format!(
            "{}{}",
            &self.content[..start_offset],
            &self.content[end_offset..]
        );

        // Clean up extra blank lines
        self.cleanup_blank_lines();

        Ok(())
    }

    /// Move a markdown cell up or down.
    ///
    /// Swaps the markdown block with the adjacent one.
    pub fn move_markdown_cell(
        &mut self,
        start_line: usize,
        end_line: usize,
        direction: MoveDirection,
    ) -> Result<()> {
        let lines: Vec<&str> = self.content.lines().collect();

        if start_line == 0 || start_line > lines.len() || end_line > lines.len() || start_line > end_line {
            return Err(Error::InvalidOperation(format!(
                "Invalid line range: {}-{}",
                start_line, end_line
            )));
        }

        // Extract the markdown block
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);
        let markdown_block = self.content[start_offset..end_offset].to_string();

        // Find the adjacent block to swap with
        let (swap_start_line, swap_end_line) = match direction {
            MoveDirection::Up => {
                // Find the previous block (scan backwards)
                if start_line == 1 {
                    return Err(Error::InvalidOperation("Cannot move first block up".to_string()));
                }

                // Simple heuristic: find previous non-empty line group
                let mut search_line = start_line - 1;
                while search_line > 0 && lines[search_line - 1].trim().is_empty() {
                    search_line -= 1;
                }

                if search_line == 0 {
                    return Err(Error::InvalidOperation("No block found above".to_string()));
                }

                // Find the start of this block
                let mut block_start = search_line;
                while block_start > 1 && !lines[block_start - 2].trim().is_empty() {
                    block_start -= 1;
                }

                (block_start, search_line)
            }
            MoveDirection::Down => {
                // Find the next block (scan forwards)
                if end_line >= lines.len() {
                    return Err(Error::InvalidOperation("Cannot move last block down".to_string()));
                }

                // Skip blank lines
                let mut search_line = end_line + 1;
                while search_line <= lines.len() && lines[search_line - 1].trim().is_empty() {
                    search_line += 1;
                }

                if search_line > lines.len() {
                    return Err(Error::InvalidOperation("No block found below".to_string()));
                }

                // Find the end of this block
                let block_start = search_line;
                let mut block_end = search_line;
                while block_end < lines.len() && !lines[block_end].trim().is_empty() {
                    block_end += 1;
                }

                (block_start, block_end)
            }
        };

        // Extract the swap block
        let swap_start_offset = self.line_start_offset(swap_start_line, &lines);
        let swap_end_offset = self.line_to_byte_offset(swap_end_line, &lines);
        let swap_block = self.content[swap_start_offset..swap_end_offset].to_string();

        // Perform the swap based on direction
        match direction {
            MoveDirection::Up => {
                // Swap block goes after markdown block
                self.content = format!(
                    "{}{}{}{}{}",
                    &self.content[..swap_start_offset],
                    &markdown_block,
                    &self.content[swap_end_offset..start_offset],
                    &swap_block,
                    &self.content[end_offset..]
                );
            }
            MoveDirection::Down => {
                // Markdown block goes after swap block
                self.content = format!(
                    "{}{}{}{}{}",
                    &self.content[..start_offset],
                    &swap_block,
                    &self.content[end_offset..swap_start_offset],
                    &markdown_block,
                    &self.content[swap_end_offset..]
                );
            }
        }

        Ok(())
    }

    /// Save the modified content back to the file.
    pub fn save(&self) -> Result<()> {
        fs::write(&self.path, &self.content)?;
        Ok(())
    }

    /// Get the source code of a cell (including doc comments and attributes).
    ///
    /// Used for undo operations to capture cell content before deletion.
    pub fn get_cell_source(&self, cell_name: &str) -> Result<String> {
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        let (start_line, end_line) = self.find_cell_span(&file, cell_name)?;

        let lines: Vec<&str> = self.content.lines().collect();
        let start_offset = self.line_start_offset(start_line, &lines);
        let end_offset = self.line_to_byte_offset(end_line, &lines);

        Ok(self.content[start_offset..end_offset].to_string())
    }

    /// Get the name of the cell that appears before the specified cell.
    ///
    /// Returns None if the cell is the first one.
    /// Used for undo operations to track position for restoration.
    pub fn get_previous_cell_name(&self, cell_name: &str) -> Result<Option<String>> {
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        let cells = self.collect_cell_spans(&file);

        let cell_idx = cells
            .iter()
            .position(|(name, _, _)| name == cell_name)
            .ok_or_else(|| Error::CellNotFound(format!("Cell '{}' not found", cell_name)))?;

        if cell_idx == 0 {
            Ok(None)
        } else {
            Ok(Some(cells[cell_idx - 1].0.clone()))
        }
    }

    /// Restore a cell with specific source code after a specific cell.
    ///
    /// If `after_cell_name` is None, inserts at the beginning (before all cells).
    /// Used for undo delete operations.
    pub fn restore_cell(&mut self, source: &str, after_cell_name: Option<&str>) -> Result<()> {
        let file: File = syn::parse_str(&self.content)
            .map_err(|e| Error::Parse(format!("Failed to parse source: {}", e)))?;

        let insert_pos = if let Some(after_name) = after_cell_name {
            // Insert after the specified cell
            self.find_insert_position(&file, Some(after_name))?
        } else {
            // Insert at the beginning - find the first cell and insert before it
            let cells = self.collect_cell_spans(&file);
            if cells.is_empty() {
                // No cells, insert at end
                self.content.len()
            } else {
                // Insert before the first cell
                let lines: Vec<&str> = self.content.lines().collect();
                self.line_start_offset(cells[0].1, &lines)
            }
        };

        // Insert the source with appropriate newlines
        let insert_code = if after_cell_name.is_some() {
            format!("\n\n{}", source.trim())
        } else {
            // Inserting at beginning
            format!("{}\n\n", source.trim())
        };

        self.content.insert_str(insert_pos, &insert_code);

        Ok(())
    }

    /// Find the span of a cell (start line to end line, 1-indexed).
    /// Includes doc comments and attributes above the function.
    fn find_cell_span(&self, file: &File, cell_name: &str) -> Result<(usize, usize)> {
        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                if Self::has_cell_attribute(&func.attrs) {
                    let name = func.sig.ident.to_string();
                    if name == cell_name {
                        // Start from the first attribute or doc comment
                        let start_line = if !func.attrs.is_empty() {
                            // Find earliest attribute/doc comment line
                            func.attrs
                                .iter()
                                .map(|a| a.span().start().line)
                                .min()
                                .unwrap_or(func.sig.fn_token.span.start().line)
                        } else {
                            func.sig.fn_token.span.start().line
                        };

                        let end_line = func.block.brace_token.span.close().end().line;

                        return Ok((start_line, end_line));
                    }
                }
            }
        }

        Err(Error::CellNotFound(format!("Cell '{}' not found", cell_name)))
    }

    /// Get the byte offset at the start of a line (1-indexed).
    fn line_start_offset(&self, line: usize, lines: &[&str]) -> usize {
        if line <= 1 {
            return 0;
        }

        let mut offset = 0;
        for (i, l) in lines.iter().enumerate() {
            if i + 1 >= line {
                break;
            }
            offset += l.len();
            offset += 1; // newline
        }

        offset.min(self.content.len())
    }

    /// Get the indentation (leading whitespace) of a line (1-indexed).
    fn get_line_indent<'a>(lines: &'a [&str], line: usize) -> &'a str {
        if line == 0 || line > lines.len() {
            return "";
        }

        let line_content = lines[line - 1];
        let trimmed = line_content.trim_start();
        &line_content[..line_content.len() - trimmed.len()]
    }

    /// Remove excessive blank lines (more than 2 consecutive).
    fn cleanup_blank_lines(&mut self) {
        let mut result = String::new();
        let mut blank_count = 0;

        for line in self.content.lines() {
            if line.trim().is_empty() {
                blank_count += 1;
                if blank_count <= 2 {
                    result.push_str(line);
                    result.push('\n');
                }
            } else {
                blank_count = 0;
                result.push_str(line);
                result.push('\n');
            }
        }

        // Preserve trailing content (file may not end with newline)
        if !self.content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        self.content = result;
    }

    /// Collect all cell function names from the file.
    fn collect_cell_names(&self, file: &File) -> HashSet<String> {
        let mut names = HashSet::new();

        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                if Self::has_cell_attribute(&func.attrs) {
                    names.insert(func.sig.ident.to_string());
                }
            }
        }

        names
    }

    /// Collect all cells with their spans in source order.
    /// Returns Vec of (name, start_line, end_line).
    fn collect_cell_spans(&self, file: &File) -> Vec<(String, usize, usize)> {
        let mut cells = Vec::new();

        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                if Self::has_cell_attribute(&func.attrs) {
                    let name = func.sig.ident.to_string();

                    // Start from the first attribute or doc comment
                    let start_line = if !func.attrs.is_empty() {
                        func.attrs
                            .iter()
                            .map(|a| a.span().start().line)
                            .min()
                            .unwrap_or(func.sig.fn_token.span.start().line)
                    } else {
                        func.sig.fn_token.span.start().line
                    };

                    let end_line = func.block.brace_token.span.close().end().line;

                    cells.push((name, start_line, end_line));
                }
            }
        }

        cells
    }

    /// Generate a unique cell name (new_cell_1, new_cell_2, etc.).
    fn generate_unique_name(&self, existing: &HashSet<String>) -> String {
        for i in 1.. {
            let name = format!("new_cell_{}", i);
            if !existing.contains(&name) {
                return name;
            }
        }
        unreachable!()
    }

    /// Generate a unique copy name (e.g., `cell_copy`, `cell_copy_2`).
    fn generate_copy_name(&self, original: &str, existing: &HashSet<String>) -> String {
        // Try `original_copy` first
        let base_copy = format!("{}_copy", original);
        if !existing.contains(&base_copy) {
            return base_copy;
        }

        // Then try `original_copy_2`, `original_copy_3`, etc.
        for i in 2.. {
            let name = format!("{}_copy_{}", original, i);
            if !existing.contains(&name) {
                return name;
            }
        }
        unreachable!()
    }

    /// Find the byte position where the new cell should be inserted.
    fn find_insert_position(&self, file: &File, after_cell_id: Option<&str>) -> Result<usize> {
        let lines: Vec<&str> = self.content.lines().collect();

        // Track the end position of cells
        let mut last_cell_end_line = 0;
        let mut target_end_line = None;

        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                if Self::has_cell_attribute(&func.attrs) {
                    let name = func.sig.ident.to_string();

                    // Get the end line of this function
                    let end_line = func.block.brace_token.span.close().end().line;

                    if let Some(target) = after_cell_id {
                        if name == target {
                            target_end_line = Some(end_line);
                            break;
                        }
                    }

                    last_cell_end_line = end_line;
                }
            }
        }

        // Determine which line to insert after
        let insert_after_line = match after_cell_id {
            Some(id) => target_end_line.ok_or_else(|| {
                Error::CellNotFound(format!("Cell '{}' not found", id))
            })?,
            None => {
                // Insert at end - if no cells, insert at end of file
                if last_cell_end_line == 0 {
                    return Ok(self.content.len());
                }
                last_cell_end_line
            }
        };

        // Convert line number to byte offset (lines are 1-indexed from syn)
        let byte_offset = self.line_to_byte_offset(insert_after_line, &lines);

        Ok(byte_offset)
    }

    /// Convert a 1-indexed line number to a byte offset (end of that line).
    fn line_to_byte_offset(&self, line: usize, lines: &[&str]) -> usize {
        if line == 0 || line > lines.len() {
            return self.content.len();
        }

        // Sum the bytes of all lines up to and including the target line
        let mut offset = 0;
        for (i, l) in lines.iter().enumerate() {
            offset += l.len();
            offset += 1; // newline character

            if i + 1 == line {
                break;
            }
        }

        offset.min(self.content.len())
    }

    /// Generate the code for a new cell.
    fn generate_cell_code(&self, name: &str) -> String {
        format!(
            r#"

/// New cell
#[venus::cell]
pub fn {}() -> String {{
    "Hello".to_string()
}}
"#,
            name
        )
    }

    /// Check if a function has the #[venus::cell] attribute.
    fn has_cell_attribute(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| {
            let path = attr.path();
            let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

            // Match #[venus::cell] or #[cell] (if imported)
            (segments.len() == 2 && segments[0] == "venus" && segments[1] == "cell")
                || (segments.len() == 1 && segments[0] == "cell")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_insert_cell_at_end() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    1
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.insert_cell(None).unwrap();
        assert_eq!(name, "new_cell_1");

        // Check that the new cell is in the content
        assert!(editor.content.contains("#[venus::cell]"));
        assert!(editor.content.contains("pub fn new_cell_1()"));
    }

    #[test]
    fn test_insert_cell_after_specific() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    1
}

/// Second cell
#[venus::cell]
pub fn second(first: &i32) -> i32 {
    *first + 1
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.insert_cell(Some("first")).unwrap();
        assert_eq!(name, "new_cell_1");

        // Verify the new cell appears after 'first' but before 'second'
        let first_pos = editor.content.find("pub fn first()").unwrap();
        let new_pos = editor.content.find("pub fn new_cell_1()").unwrap();
        let second_pos = editor.content.find("pub fn second(").unwrap();

        assert!(first_pos < new_pos);
        assert!(new_pos < second_pos);
    }

    #[test]
    fn test_unique_name_generation() {
        let source = r#"use venus::prelude::*;

#[venus::cell]
pub fn new_cell_1() -> i32 { 1 }

#[venus::cell]
pub fn new_cell_2() -> i32 { 2 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.insert_cell(None).unwrap();
        assert_eq!(name, "new_cell_3");
    }

    #[test]
    fn test_insert_into_empty_file() {
        let source = r#"use venus::prelude::*;

fn main() {}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.insert_cell(None).unwrap();
        assert_eq!(name, "new_cell_1");
        assert!(editor.content.contains("pub fn new_cell_1()"));
    }

    #[test]
    fn test_save() {
        let source = r#"#[venus::cell]
pub fn test() -> i32 { 1 }
"#;

        let file = create_temp_file(source);
        let path = file.path().to_path_buf();

        {
            let mut editor = SourceEditor::load(&path).unwrap();
            editor.insert_cell(None).unwrap();
            editor.save().unwrap();
        }

        // Read back and verify
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("pub fn new_cell_1()"));
    }

    #[test]
    fn test_delete_cell() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    1
}

/// Second cell
#[venus::cell]
pub fn second() -> i32 {
    2
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.delete_cell("first").unwrap();
        assert_eq!(name, "first");

        // Verify first cell is gone but second remains
        assert!(!editor.content.contains("pub fn first()"));
        assert!(editor.content.contains("pub fn second()"));
        // Header should remain
        assert!(editor.content.contains("use venus::prelude::*;"));
    }

    #[test]
    fn test_delete_cell_with_doc_comments() {
        let source = r#"use venus::prelude::*;

/// This is a doc comment
/// with multiple lines
#[venus::cell]
pub fn documented() -> i32 {
    42
}

#[venus::cell]
pub fn other() -> i32 {
    1
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        editor.delete_cell("documented").unwrap();

        // Verify the doc comments are also removed
        assert!(!editor.content.contains("This is a doc comment"));
        assert!(!editor.content.contains("pub fn documented()"));
        assert!(editor.content.contains("pub fn other()"));
    }

    #[test]
    fn test_delete_nonexistent_cell() {
        let source = r#"#[venus::cell]
pub fn exists() -> i32 { 1 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let result = editor.delete_cell("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_last_cell() {
        let source = r#"use venus::prelude::*;

/// Only cell
#[venus::cell]
pub fn only() -> i32 {
    1
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        editor.delete_cell("only").unwrap();

        // Should still have the use statement
        assert!(editor.content.contains("use venus::prelude::*;"));
        assert!(!editor.content.contains("pub fn only()"));
    }

    #[test]
    fn test_duplicate_cell() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    42
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.duplicate_cell("first").unwrap();
        assert_eq!(name, "first_copy");

        // Both original and copy should exist
        assert!(editor.content.contains("pub fn first()"));
        assert!(editor.content.contains("pub fn first_copy()"));
        // Copy should have same body
        assert!(editor.content.matches("42").count() == 2);
    }

    #[test]
    fn test_duplicate_cell_preserves_doc_comments() {
        let source = r#"use venus::prelude::*;

/// This is a documented cell
/// with multiple lines of docs
#[venus::cell]
pub fn documented() -> String {
    "hello".to_string()
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.duplicate_cell("documented").unwrap();
        assert_eq!(name, "documented_copy");

        // Doc comments should be duplicated
        assert_eq!(editor.content.matches("This is a documented cell").count(), 2);
        assert!(editor.content.contains("pub fn documented_copy()"));
    }

    #[test]
    fn test_duplicate_cell_unique_naming() {
        let source = r#"use venus::prelude::*;

#[venus::cell]
pub fn original() -> i32 { 1 }

#[venus::cell]
pub fn original_copy() -> i32 { 2 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let name = editor.duplicate_cell("original").unwrap();
        // Should be original_copy_2 since original_copy already exists
        assert_eq!(name, "original_copy_2");
        assert!(editor.content.contains("pub fn original_copy_2()"));
    }

    #[test]
    fn test_duplicate_nonexistent_cell() {
        let source = r#"#[venus::cell]
pub fn exists() -> i32 { 1 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let result = editor.duplicate_cell("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_move_cell_down() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    1
}

/// Second cell
#[venus::cell]
pub fn second() -> i32 {
    2
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        editor.move_cell("first", MoveDirection::Down).unwrap();

        // Second should now come before first
        let second_pos = editor.content.find("pub fn second()").unwrap();
        let first_pos = editor.content.find("pub fn first()").unwrap();
        assert!(second_pos < first_pos);
    }

    #[test]
    fn test_move_cell_up() {
        let source = r#"use venus::prelude::*;

/// First cell
#[venus::cell]
pub fn first() -> i32 {
    1
}

/// Second cell
#[venus::cell]
pub fn second() -> i32 {
    2
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        editor.move_cell("second", MoveDirection::Up).unwrap();

        // Second should now come before first
        let second_pos = editor.content.find("pub fn second()").unwrap();
        let first_pos = editor.content.find("pub fn first()").unwrap();
        assert!(second_pos < first_pos);
    }

    #[test]
    fn test_move_first_cell_up_fails() {
        let source = r#"#[venus::cell]
pub fn first() -> i32 { 1 }

#[venus::cell]
pub fn second() -> i32 { 2 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let result = editor.move_cell("first", MoveDirection::Up);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_last_cell_down_fails() {
        let source = r#"#[venus::cell]
pub fn first() -> i32 { 1 }

#[venus::cell]
pub fn second() -> i32 { 2 }
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        let result = editor.move_cell("second", MoveDirection::Down);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_preserves_doc_comments() {
        let source = r#"use venus::prelude::*;

/// This is the first cell
/// with multiple lines
#[venus::cell]
pub fn first() -> i32 {
    1
}

/// This is the second cell
#[venus::cell]
pub fn second() -> i32 {
    2
}
"#;

        let file = create_temp_file(source);
        let mut editor = SourceEditor::load(file.path()).unwrap();

        editor.move_cell("first", MoveDirection::Down).unwrap();

        // Check doc comments are preserved and in right order
        let second_doc_pos = editor.content.find("This is the second cell").unwrap();
        let first_doc_pos = editor.content.find("This is the first cell").unwrap();
        assert!(second_doc_pos < first_doc_pos);
    }
}
