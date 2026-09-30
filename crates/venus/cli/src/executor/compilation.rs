use super::{CompilationInfo, CompiledCellInfo, NotebookExecutor};
use crate::colors;
use std::collections::HashMap;
use venus_core::compile::{CellCompiler, CompilationResult};

impl NotebookExecutor {
    /// Compile all cells.
    pub fn compile(&self) -> anyhow::Result<CompilationInfo> {
        println!("\n{}Compiling cells...{}", colors::BOLD, colors::RESET);

        let compiler = CellCompiler::new(self.config.clone(), self.toolchain.clone())
            .with_universe(self.universe_path.clone());

        let mut compiled_cells = HashMap::new();
        let mut compile_errors = Vec::new();
        let deps_hash = self.universe_builder.deps_hash();

        for cell in &self.cells {
            print!("  {} {} ... ", colors::DIM, cell.name);
            colors::flush_stdout();

            let real_id = self.cell_ids[&cell.name];
            let result = compiler.compile(cell, deps_hash);

            match result {
                CompilationResult::Success(mut compiled) => {
                    let compile_time_ms = compiled.compile_time_ms;
                    compiled.cell_id = real_id;
                    println!(
                        "{}✓{} ({}ms)",
                        colors::GREEN,
                        colors::RESET,
                        compile_time_ms
                    );
                    compiled_cells.insert(
                        real_id,
                        CompiledCellInfo {
                            compiled,
                            dep_count: cell.dependencies.len(),
                            compile_time_ms,
                            cached: false,
                        },
                    );
                }
                CompilationResult::Cached(mut compiled) => {
                    compiled.cell_id = real_id;
                    println!(
                        "{}✓{} {}(cached){}",
                        colors::GREEN,
                        colors::RESET,
                        colors::DIM,
                        colors::RESET
                    );
                    compiled_cells.insert(
                        real_id,
                        CompiledCellInfo {
                            compiled,
                            dep_count: cell.dependencies.len(),
                            compile_time_ms: 0,
                            cached: true,
                        },
                    );
                }
                CompilationResult::Failed { cell_id: _, errors } => {
                    println!("{}✗{}", colors::RED, colors::RESET);
                    for error in &errors {
                        if let Some(rendered) = &error.rendered {
                            eprintln!("{}", rendered);
                        } else {
                            eprintln!("{}", error.format_terminal());
                        }
                    }
                    compile_errors.push((cell.name.clone(), errors));
                }
            }
        }

        Ok(CompilationInfo {
            cells: compiled_cells,
            errors: compile_errors,
        })
    }
}
