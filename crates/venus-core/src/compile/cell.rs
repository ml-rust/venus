//! Cell compiler for Venus notebooks.
//!
//! Compiles individual cells to dynamic libraries using Cranelift
//! for fast compilation during development.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use crate::graph::CellInfo;

use super::errors::ErrorMapper;
use super::toolchain::ToolchainManager;
use super::types::{
    CompilationResult, CompiledCell, CompilerConfig, dylib_extension, dylib_prefix,
};

/// Compiles individual cells to dynamic libraries.
pub struct CellCompiler {
    /// Compiler configuration
    config: CompilerConfig,

    /// Toolchain manager
    toolchain: ToolchainManager,

    /// Path to the universe library (for linking)
    universe_path: Option<PathBuf>,
}

impl CellCompiler {
    /// Create a new cell compiler.
    pub fn new(config: CompilerConfig, toolchain: ToolchainManager) -> Self {
        Self {
            config,
            toolchain,
            universe_path: None,
        }
    }

    /// Set the universe library path for linking.
    pub fn with_universe(mut self, path: PathBuf) -> Self {
        self.universe_path = Some(path);
        self
    }

    /// Compile a cell to a dynamic library.
    pub fn compile(&self, cell: &CellInfo, deps_hash: u64) -> CompilationResult {
        let source_hash = self.hash_source(&cell.source_code);

        // Check cache
        if let Some(cached) = self.check_cache(cell, source_hash, deps_hash) {
            return CompilationResult::Cached(cached);
        }

        let start = Instant::now();

        // Generate wrapper code
        let wrapper_code = self.generate_wrapper(cell);

        // Compile
        match self.compile_to_dylib(cell, &wrapper_code) {
            Ok(dylib_path) => {
                let compile_time = start.elapsed().as_millis() as u64;

                let compiled = CompiledCell {
                    cell_id: cell.id,
                    name: cell.name.clone(),
                    dylib_path,
                    entry_symbol: format!("venus_cell_{}", cell.name),
                    source_hash,
                    deps_hash,
                    compile_time_ms: compile_time,
                };

                // Save to cache
                self.save_to_cache(&compiled);

                CompilationResult::Success(compiled)
            }
            Err(errors) => CompilationResult::Failed {
                cell_id: cell.id,
                errors,
            },
        }
    }

    /// Generate the wrapper code for a cell.
    fn generate_wrapper(&self, cell: &CellInfo) -> String {
        let mut code = String::new();

        // Header
        code.push_str("// Auto-generated cell wrapper\n");
        code.push_str("#![allow(unused_imports)]\n");
        code.push_str("#![allow(dead_code)]\n\n");

        // Import dependencies from universe (always built, includes rkyv)
        // NOTE: venus_universe includes user-defined types from the notebook,
        // external dependencies, and rkyv. The glob import is safe because:
        // 1. User types are defined in the notebook itself
        // 2. rkyv::rancor::Error is aliased as RkyvError to avoid conflicts
        // 3. Cells can shadow imports locally if needed
        code.push_str("extern crate venus_universe;\n");
        code.push_str("use venus_universe::*;\n\n");

        // Comment with source location for error mapping (not a real directive)
        code.push_str(&format!(
            "// Original source: {}:{}\n",
            cell.source_file.display(),
            cell.span.start_line
        ));

        // The cell function itself (from source)
        code.push_str(&cell.source_code);
        code.push_str("\n\n");

        // Generate FFI entry point
        code.push_str(&self.generate_ffi_entry(cell));

        code
    }

    /// Generate the FFI entry point for a cell.
    fn generate_ffi_entry(&self, cell: &CellInfo) -> String {
        let mut code = String::new();

        let fn_name = &cell.name;
        let entry_name = format!("venus_cell_{}", fn_name);

        // Determine return handling
        let returns_result = cell.return_type.starts_with("Result<");

        code.push_str("/// FFI entry point for the cell.\n");
        code.push_str("/// \n");
        code.push_str("/// # Safety\n");
        code.push_str("/// This function is called from the Venus runtime.\n");
        code.push_str("#[no_mangle]\n");
        code.push_str(&format!("pub unsafe extern \"C\" fn {}(\n", entry_name));

        // Input parameters (serialized)
        for (i, dep) in cell.dependencies.iter().enumerate() {
            code.push_str(&format!("    {}_ptr: *const u8,\n", dep.param_name));
            code.push_str(&format!("    {}_len: usize,\n", dep.param_name));
            if i < cell.dependencies.len() - 1 {
                code.push('\n');
            }
        }

        // Widget values input
        code.push_str("    widget_values_ptr: *const u8,\n");
        code.push_str("    widget_values_len: usize,\n");

        // Output parameters
        code.push_str("    out_ptr: *mut *mut u8,\n");
        code.push_str("    out_len: *mut usize,\n");
        code.push_str(") -> i32 {\n");

        // Set up widget context with incoming values
        code.push_str("    // Set up widget context\n");
        code.push_str("    use std::collections::HashMap;\n");
        code.push_str("    let widget_values: HashMap<String, WidgetValue> = if widget_values_len > 0 {\n");
        code.push_str("        let json_slice = std::slice::from_raw_parts(widget_values_ptr, widget_values_len);\n");
        code.push_str("        venus_universe::serde_json::from_slice(json_slice).unwrap_or_default()\n");
        code.push_str("    } else {\n");
        code.push_str("        HashMap::new()\n");
        code.push_str("    };\n");
        code.push_str("    set_widget_context(WidgetContext::with_values(widget_values));\n\n");

        // Deserialize inputs using rkyv (zero-copy access then deserialize)
        for dep in &cell.dependencies {
            // Get the base type without reference
            let base_type = dep.param_type.trim_start_matches('&').trim();

            code.push_str(&format!(
                "    let {}_bytes = std::slice::from_raw_parts({}_ptr, {}_len);\n",
                dep.param_name, dep.param_name, dep.param_name
            ));
            // Access archived data (zero-copy)
            code.push_str(&format!(
                "    let {}_archived = match rkyv::access::<rkyv::Archived<{}>, RkyvError>({}_bytes) {{\n",
                dep.param_name, base_type, dep.param_name
            ));
            code.push_str("        Ok(v) => v,\n");
            code.push_str("        Err(_) => return -1, // Access error\n");
            code.push_str("    };\n");
            // Deserialize to owned type
            code.push_str(&format!(
                "    let {}: {} = match rkyv::deserialize::<_, RkyvError>({}_archived) {{\n",
                dep.param_name, base_type, dep.param_name
            ));
            code.push_str("        Ok(v) => v,\n");
            code.push_str("        Err(_) => return -1, // Deserialization error\n");
            code.push_str("    };\n\n");
        }

        // Build argument list for cell call
        let args: Vec<String> = cell
            .dependencies
            .iter()
            .map(|d| {
                if d.is_ref {
                    if d.is_mut {
                        format!("&mut {}", d.param_name)
                    } else {
                        format!("&{}", d.param_name)
                    }
                } else {
                    d.param_name.clone()
                }
            })
            .collect();

        // Wrap cell execution in catch_unwind for panic safety.
        // This prevents user code panics from crashing the Venus server.
        code.push_str("    // Wrap execution in catch_unwind for panic safety\n");
        code.push_str("    let execution_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {\n");

        // Call the cell function (inside catch_unwind)
        if returns_result {
            code.push_str(&format!(
                "        let result = match {}({}) {{\n",
                fn_name,
                args.join(", ")
            ));
            code.push_str("            Ok(v) => v,\n");
            code.push_str("            Err(_) => return Err(-2i32), // Cell returned error\n");
            code.push_str("        };\n\n");
        } else {
            code.push_str(&format!(
                "        let result = {}({});\n\n",
                fn_name,
                args.join(", ")
            ));
        }

        // Create debug display string (inside catch_unwind)
        code.push_str("        let display_str = format!(\"{:?}\", result);\n");
        code.push_str("        let display_bytes = display_str.as_bytes();\n\n");

        // Serialize output with rkyv (inside catch_unwind)
        code.push_str("        let rkyv_data = match rkyv::to_bytes::<RkyvError>(&result) {\n");
        code.push_str("            Ok(v) => v,\n");
        code.push_str("            Err(_) => return Err(-3i32), // Serialization error\n");
        code.push_str("        };\n\n");

        // Capture widgets from context (inside catch_unwind, after cell execution)
        code.push_str("        // Capture registered widgets\n");
        code.push_str("        let widgets_json = if let Some(mut ctx) = take_widget_context() {\n");
        code.push_str("            let widgets = ctx.take_widgets();\n");
        code.push_str("            if widgets.is_empty() { Vec::new() } else { venus_universe::serde_json::to_vec(&widgets).unwrap_or_default() }\n");
        code.push_str("        } else { Vec::new() };\n\n");

        // Format: display_len (8 bytes LE) | display_bytes | widgets_len (8 bytes LE) | widgets_json | rkyv_data
        code.push_str("        let display_len = display_bytes.len() as u64;\n");
        code.push_str("        let widgets_len = widgets_json.len() as u64;\n");
        code.push_str("        let total_len = 8 + display_bytes.len() + 8 + widgets_json.len() + rkyv_data.len();\n");
        code.push_str("        let mut output = Vec::with_capacity(total_len);\n");
        code.push_str("        output.extend_from_slice(&display_len.to_le_bytes());\n");
        code.push_str("        output.extend_from_slice(display_bytes);\n");
        code.push_str("        output.extend_from_slice(&widgets_len.to_le_bytes());\n");
        code.push_str("        output.extend_from_slice(&widgets_json);\n");
        code.push_str("        output.extend_from_slice(&rkyv_data);\n\n");
        code.push_str("        Ok(output)\n");
        code.push_str("    }));\n\n");

        // Handle catch_unwind result
        code.push_str("    // Handle panic or success\n");
        code.push_str("    match execution_result {\n");
        code.push_str("        Ok(Ok(output)) => {\n");
        code.push_str("            let len = output.len();\n");
        code.push_str("            let ptr = output.as_ptr();\n");
        code.push_str("            std::mem::forget(output);\n");
        code.push_str("            *out_ptr = ptr as *mut u8;\n");
        code.push_str("            *out_len = len;\n");
        code.push_str("            0 // Success\n");
        code.push_str("        }\n");
        code.push_str("        Ok(Err(code)) => code, // Cell error or serialization error\n");
        code.push_str("        Err(_) => -4, // Panic occurred\n");
        code.push_str("    }\n");
        code.push_str("}\n");

        code
    }

    /// Compile wrapper code to a dynamic library.
    fn compile_to_dylib(
        &self,
        cell: &CellInfo,
        wrapper_code: &str,
    ) -> std::result::Result<PathBuf, Vec<super::CompileError>> {
        let build_dir = self.config.cell_build_dir();
        fs::create_dir_all(&build_dir).map_err(|e| {
            super::CompileError::simple(format!("Failed to create build directory: {}", e))
        })?;

        // Write wrapper source
        let src_file = build_dir.join(format!("{}.rs", cell.name));
        fs::write(&src_file, wrapper_code)
            .map_err(|e| super::CompileError::simple(format!("Failed to write source: {}", e)))?;

        // Output path
        let dylib_name = format!("{}cell_{}.{}", dylib_prefix(), cell.name, dylib_extension());
        let dylib_path = build_dir.join(&dylib_name);

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
            for flag in self.toolchain.cranelift_flags() {
                cmd.arg(&flag);
            }
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
                cmd.arg("--extern").arg(format!("venus_universe={}", rlib_path.display()));
            } else {
                // Fallback: try to find it in deps
                if let Ok(entries) = std::fs::read_dir(&deps_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("libvenus_universe-") && name_str.ends_with(".rlib") {
                            cmd.arg("--extern").arg(format!("venus_universe={}", entry.path().display()));
                            break;
                        }
                    }
                }
            }

            // Add rpath for runtime linking (Unix-like systems)
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                // Runtime links against cdylib in the universe build dir
                cmd.arg(format!("-Clink-arg=-Wl,-rpath,{}", universe_build_dir.display()));
            }
        }

        // Extra flags
        for flag in &self.config.extra_rustc_flags {
            cmd.arg(flag);
        }

        // Run compilation
        let output = cmd
            .output()
            .map_err(|e| super::CompileError::simple(format!("Failed to run rustc: {}", e)))?;

        if output.status.success() {
            Ok(dylib_path)
        } else {
            // Parse errors
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mapper = ErrorMapper::new(cell.source_file.clone());
            let errors = mapper.parse_rustc_output(&stderr);

            if errors.is_empty() {
                // Fallback if JSON parsing failed
                Err(super::CompileError::simple_rendered(stderr.to_string()))
            } else {
                Err(errors)
            }
        }
    }

    /// Hash the source code.
    fn hash_source(&self, source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        hasher.finish()
    }

    /// Check if a cached compilation exists.
    fn check_cache(
        &self,
        cell: &CellInfo,
        source_hash: u64,
        deps_hash: u64,
    ) -> Option<CompiledCell> {
        let cache_file = self.cache_path(cell);
        if !cache_file.exists() {
            return None;
        }

        // Read cache metadata
        let meta_file = self.cache_meta_path(&cell.name);
        if let Ok(meta) = fs::read_to_string(&meta_file) {
            let lines: Vec<&str> = meta.lines().collect();
            if lines.len() >= 2
                && let (Ok(cached_src), Ok(cached_deps)) =
                    (lines[0].parse::<u64>(), lines[1].parse::<u64>())
                && cached_src == source_hash
                && cached_deps == deps_hash
            {
                return Some(CompiledCell {
                    cell_id: cell.id,
                    name: cell.name.clone(),
                    dylib_path: cache_file,
                    entry_symbol: format!("venus_cell_{}", cell.name),
                    source_hash,
                    deps_hash,
                    compile_time_ms: 0,
                });
            }
        }

        None
    }

    /// Save compilation result to cache.
    fn save_to_cache(&self, compiled: &CompiledCell) {
        let meta_file = self.cache_meta_path(&compiled.name);

        // Ensure cache directory exists
        if let Some(parent) = meta_file.parent()
            && let Err(e) = fs::create_dir_all(parent) {
                tracing::warn!("Failed to create cache directory: {}", e);
                return;
            }

        let meta = format!("{}\n{}", compiled.source_hash, compiled.deps_hash);
        // Cache save is opportunistic; failure doesn't affect correctness
        if let Err(e) = fs::write(&meta_file, meta) {
            tracing::warn!("Failed to save cell cache: {}", e);
        }
    }

    /// Get the cache path for a cell.
    fn cache_path(&self, cell: &CellInfo) -> PathBuf {
        let filename = format!("{}cell_{}.{}", dylib_prefix(), cell.name, dylib_extension());
        self.config.cache_dir.join("cells").join(filename)
    }

    /// Get the cache metadata path by cell name.
    fn cache_meta_path(&self, name: &str) -> PathBuf {
        self.config
            .cache_dir
            .join("cells")
            .join(format!("{}.meta", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CellId, Dependency, SourceSpan};

    fn make_test_cell() -> CellInfo {
        CellInfo {
            id: CellId::new(0),
            name: "test_cell".to_string(),
            display_name: "test_cell".to_string(),
            dependencies: vec![],
            return_type: "i32".to_string(),
            doc_comment: None,
            source_code: "pub fn test_cell() -> i32 { 42 }".to_string(),
            source_file: PathBuf::from("test.rs"),
            span: SourceSpan {
                start_line: 1,
                start_col: 0,
                end_line: 1,
                end_col: 30,
            },
        }
    }

    #[test]
    fn test_generate_wrapper_simple() {
        let config = CompilerConfig::default();
        let toolchain = ToolchainManager::new().unwrap();
        let compiler = CellCompiler::new(config, toolchain);

        let cell = make_test_cell();
        let wrapper = compiler.generate_wrapper(&cell);

        assert!(wrapper.contains("venus_cell_test_cell"));
        assert!(wrapper.contains("pub fn test_cell() -> i32"));
        assert!(wrapper.contains("#[no_mangle]"));
    }

    #[test]
    fn test_generate_wrapper_with_deps() {
        let config = CompilerConfig::default();
        let toolchain = ToolchainManager::new().unwrap();
        let compiler = CellCompiler::new(config, toolchain);

        let cell = CellInfo {
            id: CellId::new(1),
            name: "process".to_string(),
            display_name: "process".to_string(),
            dependencies: vec![Dependency {
                param_name: "config".to_string(),
                param_type: "Config".to_string(),
                is_ref: true,
                is_mut: false,
            }],
            return_type: "Output".to_string(),
            doc_comment: None,
            source_code: "pub fn process(config: &Config) -> Output { todo!() }".to_string(),
            source_file: PathBuf::from("test.rs"),
            span: SourceSpan {
                start_line: 5,
                start_col: 0,
                end_line: 5,
                end_col: 50,
            },
        };

        let wrapper = compiler.generate_wrapper(&cell);

        assert!(wrapper.contains("config_ptr: *const u8"));
        assert!(wrapper.contains("config_len: usize"));
        assert!(wrapper.contains("rkyv::access"));
    }

    #[test]
    fn test_hash_source() {
        let config = CompilerConfig::default();
        let toolchain = ToolchainManager::new().unwrap();
        let compiler = CellCompiler::new(config, toolchain);

        let hash1 = compiler.hash_source("fn foo() {}");
        let hash2 = compiler.hash_source("fn foo() {}");
        let hash3 = compiler.hash_source("fn bar() {}");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
