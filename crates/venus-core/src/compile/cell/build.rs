use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::graph::CellInfo;

use super::super::errors::{CompileError, ErrorMapper};
use super::super::types::{dylib_extension, dylib_prefix};
use super::compiler::CellCompiler;

impl CellCompiler {
    /// Compile wrapper code to a dynamic library.
    pub(super) fn compile_to_dylib(
        &self,
        cell: &CellInfo,
        wrapper_code: &str,
        source_hash: u64,
        deps_hash: u64,
    ) -> std::result::Result<PathBuf, Vec<CompileError>> {
        let build_dir = self.config.cell_build_dir();
        fs::create_dir_all(&build_dir).map_err(|e| {
            CompileError::simple(format!("Failed to create build directory: {}", e))
        })?;

        // Write wrapper source
        let src_file = build_dir.join(format!("{}.rs", cell.name));
        fs::write(&src_file, wrapper_code)
            .map_err(|e| CompileError::simple(format!("Failed to write source: {}", e)))?;

        // Source and dependency hashes distinguish loaded library versions.
        let dylib_name = format!(
            "{}cell_{}_{:x}_{:x}.{}",
            dylib_prefix(),
            cell.name,
            source_hash,
            deps_hash,
            dylib_extension()
        );
        let dylib_path = build_dir.join(&dylib_name);

        // Clean up old dylibs for this cell (they accumulate with different hashes)
        let cell_prefix = format!("{}cell_{}_", dylib_prefix(), cell.name);
        if let Ok(entries) = fs::read_dir(&build_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(&cell_prefix) && name_str != dylib_name {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        // Build rustc command
        let mut cmd = Command::new(self.toolchain.rustc_path());

        cmd.arg(&src_file)
            .arg("--crate-type=cdylib")
            .arg("--edition=2021")
            .arg("-o")
            .arg(&dylib_path)
            .arg("--error-format=json");

        // Add Cranelift backend if available and configured
        if self.config.use_cranelift && self.toolchain.has_cranelift() {
            cmd.args(self.toolchain.cranelift_flags());
        }

        // Optimization level
        cmd.arg(format!("-Copt-level={}", self.config.opt_level));

        // Debug info
        if self.config.debug_info {
            cmd.arg("-g");
        }

        // Link against universe rlib for compilation
        if let Some(universe_dylib) = &self.universe_path {
            // The universe build directory contains both cdylib and rlib
            // We need the rlib for rustc compilation and cdylib for runtime
            let universe_build_dir = universe_dylib.parent().unwrap_or(universe_dylib);
            let target_release_dir = universe_build_dir.join("target").join("release");
            let deps_dir = target_release_dir.join("deps");

            // Add search paths for dependencies
            cmd.arg("-L").arg(&target_release_dir);
            cmd.arg("-L").arg(&deps_dir);

            // Find and link the universe rlib using --extern
            let rlib_path = target_release_dir.join("libvenus_universe.rlib");
            if rlib_path.exists() {
                cmd.arg("--extern")
                    .arg(format!("venus_universe={}", rlib_path.display()));
            } else {
                // Fallback: try to find it in deps
                if let Ok(entries) = std::fs::read_dir(&deps_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("libvenus_universe-") && name_str.ends_with(".rlib")
                        {
                            cmd.arg("--extern")
                                .arg(format!("venus_universe={}", entry.path().display()));
                            break;
                        }
                    }
                }
            }

            // Add rpath for runtime linking (Unix-like systems)
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                // Runtime links against cdylib in the universe build dir
                cmd.arg(format!(
                    "-Clink-arg=-Wl,-rpath,{}",
                    universe_build_dir.display()
                ));
            }

            // On macOS, fix the universe dylib install_name so the dynamic
            // linker can resolve it via rpath. Raw rustc sets install_name to
            // the bare filename, but @rpath/ prefix is needed for rpath lookup.
            #[cfg(target_os = "macos")]
            {
                let universe_filename = universe_dylib
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let desired_install_name = format!("@rpath/{universe_filename}");

                // Check if we need to fix install_name (cargo sets it correctly,
                // but direct rustc compilation does not)
                let output = Command::new("otool")
                    .args(["-D", &universe_dylib.to_string_lossy()])
                    .output();
                if let Ok(output) = output {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if !stdout.contains("@rpath") {
                        let _ = Command::new("install_name_tool")
                            .args([
                                "-id",
                                &desired_install_name,
                                &universe_dylib.to_string_lossy(),
                            ])
                            .status();
                    }
                }
            }
        }

        // Extra flags
        cmd.args(&self.config.extra_rustc_flags);

        // Run compilation
        let output = cmd
            .output()
            .map_err(|e| CompileError::simple(format!("Failed to run rustc: {}", e)))?;

        if output.status.success() {
            Ok(dylib_path)
        } else {
            // Parse errors
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mapper = ErrorMapper::new(cell.source_file.clone());
            let errors = mapper.parse_rustc_output(&stderr);

            if errors.is_empty() {
                // Fallback if JSON parsing failed
                Err(CompileError::simple_rendered(stderr.to_string()))
            } else {
                Err(errors)
            }
        }
    }
}
