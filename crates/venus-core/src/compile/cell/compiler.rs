//! Cell compiler for Venus notebooks.
//!
//! Compiles individual cells to dynamic libraries using Cranelift
//! for fast compilation during development.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

use crate::graph::CellInfo;

use super::super::toolchain::ToolchainManager;
use super::super::types::{
    CompilationResult, CompiledCell, CompilerConfig, dylib_extension, dylib_prefix,
};

/// Compiles individual cells to dynamic libraries.
pub struct CellCompiler {
    /// Compiler configuration
    pub(super) config: CompilerConfig,

    /// Toolchain manager
    pub(super) toolchain: ToolchainManager,

    /// Path to the universe library (for linking)
    pub(super) universe_path: Option<PathBuf>,
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

        // Compile with source and dependency identity.
        match self.compile_to_dylib(cell, &wrapper_code, source_hash, deps_hash) {
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
        code.push_str(
            "    let widget_values: HashMap<String, WidgetValue> = if widget_values_len > 0 {\n",
        );
        code.push_str("        let json_slice = std::slice::from_raw_parts(widget_values_ptr, widget_values_len);\n");
        code.push_str(
            "        venus_universe::serde_json::from_slice(json_slice).unwrap_or_default()\n",
        );
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
        code.push_str(
            "        let widgets_json = if let Some(mut ctx) = take_widget_context() {\n",
        );
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
            let mut lines = meta.lines();
            if let (Some(cached_src), Some(cached_deps)) = (lines.next(), lines.next())
                && let (Ok(cached_src), Ok(cached_deps)) =
                    (cached_src.parse::<u64>(), cached_deps.parse::<u64>())
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
            && let Err(e) = fs::create_dir_all(parent)
        {
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
#[path = "tests.rs"]
mod tests;
