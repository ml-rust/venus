use super::NotebookExecutor;
use crate::colors;
use venus_core::graph::{CellId, CellInfo};

impl NotebookExecutor {
    /// Get the notebook name (file stem).
    pub fn notebook_name(&self) -> String {
        self.notebook_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }

    /// Get cell info by ID.
    pub fn cell_by_id(&self, cell_id: CellId) -> Option<&CellInfo> {
        self.cells
            .iter()
            .find(|c| self.cell_ids[&c.name] == cell_id)
    }

    /// Print a setup step.
    pub(super) fn print_step(name: &str) {
        print!("{}  ◆ {}{} ... ", colors::BLUE, name, colors::RESET);
        colors::flush_stdout();
    }

    /// Print success for a step.
    pub(super) fn print_success(extra: Option<&str>) {
        match extra {
            Some(s) => println!("{}✓{} ({})", colors::GREEN, colors::RESET, s),
            None => println!("{}✓{}", colors::GREEN, colors::RESET),
        }
    }

    /// Print the header for a run.
    pub fn print_header(&self, action: &str) {
        println!(
            "\n{}Venus{} - {} {}{}{}",
            colors::BOLD,
            colors::RESET,
            action,
            colors::CYAN,
            self.notebook_name(),
            colors::RESET
        );
        println!("{}", "─".repeat(50));
    }
}
