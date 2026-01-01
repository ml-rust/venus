//! Cargo manifest generation utilities.
//!
//! Shared logic for generating Cargo.toml files for Universe and production builds.

use std::path::Path;

use super::ExternalDependency;

/// Options for the release profile in Cargo.toml.
#[derive(Debug, Clone)]
pub struct ReleaseProfile {
    /// Optimization level (0-3).
    pub opt_level: u8,
    /// Enable Link-Time Optimization.
    pub lto: bool,
    /// Number of codegen units.
    pub codegen_units: u32,
    /// Panic strategy ("unwind" or "abort").
    pub panic: &'static str,
}

impl Default for ReleaseProfile {
    fn default() -> Self {
        Self {
            opt_level: 3,
            lto: false,
            codegen_units: 16,
            panic: "unwind",
        }
    }
}

impl ReleaseProfile {
    /// Create a profile optimized for production binaries.
    pub fn production() -> Self {
        Self {
            opt_level: 3,
            lto: true,
            codegen_units: 1,
            panic: "abort",
        }
    }
}

/// Configuration for generating a Cargo manifest.
#[derive(Debug, Clone)]
pub struct ManifestConfig<'a> {
    /// Package name.
    pub name: &'a str,
    /// Package version.
    pub version: &'a str,
    /// Rust edition.
    pub edition: &'a str,
    /// Library crate types (if building a library).
    pub lib_crate_types: Option<&'a [&'a str]>,
    /// Release profile settings.
    pub release_profile: Option<ReleaseProfile>,
    /// Whether to add an empty [workspace] table.
    pub standalone_workspace: bool,
}

impl<'a> Default for ManifestConfig<'a> {
    fn default() -> Self {
        Self {
            name: "generated",
            version: "0.1.0",
            edition: "2021",
            lib_crate_types: None,
            release_profile: None,
            standalone_workspace: false,
        }
    }
}

/// Generate a Cargo.toml manifest.
///
/// # Arguments
///
/// * `config` - Manifest configuration
/// * `dependencies` - List of dependencies to include
/// * `always_include_serde` - Whether to include serde/bincode (for production builds where user code may need them)
/// * `notebook_dir` - Base directory for resolving relative path dependencies
pub fn generate_cargo_toml(
    config: &ManifestConfig<'_>,
    dependencies: &[ExternalDependency],
    always_include_serde: bool,
    notebook_dir: Option<&Path>,
) -> String {
    let mut toml = String::new();

    // Package section
    toml.push_str("[package]\n");
    toml.push_str(&format!("name = \"{}\"\n", config.name));
    toml.push_str(&format!("version = \"{}\"\n", config.version));
    toml.push_str(&format!("edition = \"{}\"\n", config.edition));
    toml.push('\n');

    // Library section (if applicable)
    if let Some(crate_types) = config.lib_crate_types {
        toml.push_str("[lib]\n");
        let types: Vec<_> = crate_types.iter().map(|t| format!("\"{}\"", t)).collect();
        toml.push_str(&format!("crate-type = [{}]\n", types.join(", ")));
        toml.push('\n');
    }

    // Release profile (if applicable)
    if let Some(profile) = &config.release_profile {
        toml.push_str("[profile.release]\n");
        toml.push_str(&format!("opt-level = {}\n", profile.opt_level));
        if profile.lto {
            toml.push_str("lto = true\n");
        }
        toml.push_str(&format!("codegen-units = {}\n", profile.codegen_units));
        toml.push_str(&format!("panic = \"{}\"\n", profile.panic));
        toml.push('\n');
    }

    // Dependencies section
    toml.push_str("[dependencies]\n");

    // Include serde/bincode for user code that might use them (production builds only).
    // Note: Interactive execution uses rkyv via universe.rs, not this code path.
    if always_include_serde {
        toml.push_str("bincode = \"1.3\"\n");
        toml.push_str("serde = { version = \"1.0\", features = [\"derive\"] }\n");
    }

    // Add user dependencies
    for dep in dependencies {
        // Skip serde/bincode if we already added them
        if always_include_serde && (dep.name == "serde" || dep.name == "bincode") {
            continue;
        }

        format_dependency(&mut toml, dep, notebook_dir);
    }

    // Standalone workspace table (prevents being part of parent workspace)
    if config.standalone_workspace {
        toml.push('\n');
        toml.push_str("[workspace]\n");
    }

    toml
}

/// Format a single external dependency entry.
fn format_dependency(toml: &mut String, dep: &ExternalDependency, notebook_dir: Option<&Path>) {
    if let Some(path) = &dep.path {
        // Convert relative paths to absolute if notebook_dir is provided
        let abs_path = if path.is_relative() {
            notebook_dir
                .map(|dir| dir.join(path))
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| path.clone())
        } else {
            path.clone()
        };

        toml.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            dep.name,
            abs_path.display()
        ));
    } else if let Some(version) = &dep.version {
        if dep.features.is_empty() {
            toml.push_str(&format!("{} = \"{}\"\n", dep.name, version));
        } else {
            let features: Vec<_> = dep.features.iter().map(|f| format!("\"{}\"", f)).collect();
            toml.push_str(&format!(
                "{} = {{ version = \"{}\", features = [{}] }}\n",
                dep.name,
                version,
                features.join(", ")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_deps() -> Vec<ExternalDependency> {
        vec![
            ExternalDependency {
                name: "tokio".to_string(),
                version: Some("1".to_string()),
                features: vec!["full".to_string()],
                path: None,
            },
            ExternalDependency {
                name: "anyhow".to_string(),
                version: Some("1.0".to_string()),
                features: vec![],
                path: None,
            },
        ]
    }

    #[test]
    fn test_basic_manifest() {
        let config = ManifestConfig {
            name: "my_crate",
            ..Default::default()
        };
        let deps = make_deps();
        let toml = generate_cargo_toml(&config, &deps, false, None);

        assert!(toml.contains("[package]"));
        assert!(toml.contains("name = \"my_crate\""));
        assert!(toml.contains("tokio = { version = \"1\", features = [\"full\"] }"));
        assert!(toml.contains("anyhow = \"1.0\""));
    }

    #[test]
    fn test_with_serde() {
        let config = ManifestConfig::default();
        let deps = vec![];
        let toml = generate_cargo_toml(&config, &deps, true, None);

        assert!(toml.contains("bincode = \"1.3\""));
        assert!(toml.contains("serde = { version = \"1.0\", features = [\"derive\"] }"));
    }

    #[test]
    fn test_with_lib_crate_types() {
        let config = ManifestConfig {
            lib_crate_types: Some(&["cdylib", "rlib"]),
            ..Default::default()
        };
        let toml = generate_cargo_toml(&config, &[], false, None);

        assert!(toml.contains("[lib]"));
        assert!(toml.contains("crate-type = [\"cdylib\", \"rlib\"]"));
    }

    #[test]
    fn test_with_release_profile() {
        let config = ManifestConfig {
            release_profile: Some(ReleaseProfile::production()),
            ..Default::default()
        };
        let toml = generate_cargo_toml(&config, &[], false, None);

        assert!(toml.contains("[profile.release]"));
        assert!(toml.contains("opt-level = 3"));
        assert!(toml.contains("lto = true"));
        assert!(toml.contains("codegen-units = 1"));
        assert!(toml.contains("panic = \"abort\""));
    }

    #[test]
    fn test_standalone_workspace() {
        let config = ManifestConfig {
            standalone_workspace: true,
            ..Default::default()
        };
        let toml = generate_cargo_toml(&config, &[], false, None);

        assert!(toml.contains("[workspace]"));
    }

    #[test]
    fn test_path_dependency() {
        let deps = vec![ExternalDependency {
            name: "local_crate".to_string(),
            version: None,
            features: vec![],
            path: Some(PathBuf::from("/absolute/path/to/crate")),
        }];
        let toml = generate_cargo_toml(&ManifestConfig::default(), &deps, false, None);

        assert!(toml.contains("local_crate = { path = \"/absolute/path/to/crate\" }"));
    }

    #[test]
    fn test_skips_duplicate_serde() {
        let deps = vec![ExternalDependency {
            name: "serde".to_string(),
            version: Some("1.0".to_string()),
            features: vec!["derive".to_string()],
            path: None,
        }];
        let toml = generate_cargo_toml(&ManifestConfig::default(), &deps, true, None);

        // Should only have one serde entry (the auto-added one)
        let serde_count = toml.matches("serde").count();
        assert_eq!(serde_count, 1); // Only the auto-added one
    }
}
