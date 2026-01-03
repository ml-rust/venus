//! Cargo.toml manifest management for notebooks.
//!
//! Handles creating and updating Cargo.toml files to enable rust-analyzer
//! LSP support for Venus notebooks.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Configuration for how to integrate the notebook with Cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationMode {
    /// Add as [[bin]] entry to existing/new Cargo.toml (default)
    Binary,
    /// Create workspace member in separate directory
    WorkspaceMember,
}

/// Represents the type of existing Cargo.toml
#[derive(Debug, Clone, PartialEq, Eq)]
enum ManifestType {
    /// No Cargo.toml exists
    None,
    /// Cargo.toml with [package] (single crate)
    Package,
    /// Cargo.toml with [workspace] (workspace root)
    Workspace,
}

/// Manages Cargo.toml for notebook integration.
pub struct CargoManager {
    /// Directory where Cargo.toml lives or should be created
    manifest_dir: PathBuf,
    /// Path to venus crate (for dependency)
    venus_path: PathBuf,
}

impl CargoManager {
    /// Create a new CargoManager for the given directory.
    ///
    /// Automatically detects the venus crate location.
    pub fn new(manifest_dir: impl AsRef<Path>) -> Result<Self> {
        let manifest_dir = manifest_dir.as_ref().to_path_buf();
        let venus_path = Self::find_venus_crate()?;

        Ok(Self {
            manifest_dir,
            venus_path,
        })
    }

    /// Find the venus crate path.
    ///
    /// Tries in order:
    /// 1. VENUS_PATH environment variable
    /// 2. Look for venus in cargo registry (~/.cargo/registry)
    /// 3. Assume development mode (relative to CLI binary)
    fn find_venus_crate() -> Result<PathBuf> {
        // 1. Check environment variable
        if let Ok(path) = std::env::var("VENUS_PATH") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
        }

        // 2. Check if venus is installed via cargo install
        // When installed, venus-cli is in ~/.cargo/bin/venus
        // We can reference venus from crates.io
        if let Ok(exe_path) = std::env::current_exe() {
            if exe_path.starts_with(dirs::home_dir().unwrap_or_default().join(".cargo/bin")) {
                // Installed via cargo install - use crates.io version
                return Ok(PathBuf::from("venus")); // This will use registry version
            }
        }

        // 3. Development mode - find relative to this binary
        if let Ok(exe_path) = std::env::current_exe() {
            // Assume: target/release/venus or target/debug/venus
            if let Some(target_dir) = exe_path.parent().and_then(|p| p.parent()) {
                let venus_crate = target_dir.parent()
                    .map(|repo_root| repo_root.join("crates/venus"));

                if let Some(path) = venus_crate {
                    if path.exists() {
                        return Ok(path);
                    }
                }
            }
        }

        bail!("Could not find venus crate. Set VENUS_PATH environment variable.")
    }

    /// Detect the type of existing Cargo.toml (if any).
    fn detect_manifest_type(&self) -> Result<ManifestType> {
        let manifest_path = self.manifest_dir.join("Cargo.toml");

        if !manifest_path.exists() {
            return Ok(ManifestType::None);
        }

        let content = fs::read_to_string(&manifest_path)
            .context("Failed to read Cargo.toml")?;

        // Simple detection based on section headers
        if content.contains("[workspace]") {
            Ok(ManifestType::Workspace)
        } else if content.contains("[package]") {
            Ok(ManifestType::Package)
        } else {
            bail!("Invalid Cargo.toml: missing [package] or [workspace]")
        }
    }

    /// Add a notebook to the Cargo manifest.
    ///
    /// # Arguments
    ///
    /// * `notebook_name` - Name of the notebook (without .rs extension)
    /// * `notebook_path` - Relative path to the .rs file
    /// * `mode` - Integration mode (Binary or WorkspaceMember)
    pub fn add_notebook(
        &self,
        notebook_name: &str,
        notebook_path: &Path,
        mode: IntegrationMode,
    ) -> Result<()> {
        // Validate notebook name
        Self::validate_crate_name(notebook_name)?;

        let manifest_type = self.detect_manifest_type()?;

        match (manifest_type, mode) {
            // No Cargo.toml exists - create new one
            (ManifestType::None, IntegrationMode::Binary) => {
                self.create_bin_manifest(notebook_name, notebook_path)?;
            }
            (ManifestType::None, IntegrationMode::WorkspaceMember) => {
                self.create_workspace_manifest(notebook_name)?;
            }

            // Existing package - add bin
            (ManifestType::Package, IntegrationMode::Binary) => {
                self.add_bin_to_manifest(notebook_name, notebook_path)?;
            }
            (ManifestType::Package, IntegrationMode::WorkspaceMember) => {
                bail!("Cannot create workspace member: Cargo.toml is a package, not a workspace. \
                       Convert it to a workspace first or use binary mode (remove --workspace flag).");
            }

            // Existing workspace - add member
            (ManifestType::Workspace, IntegrationMode::Binary) => {
                bail!("Cannot add binary: Cargo.toml is a workspace root. \
                       Use --workspace flag to add as workspace member.");
            }
            (ManifestType::Workspace, IntegrationMode::WorkspaceMember) => {
                self.add_workspace_member(notebook_name)?;
            }
        }

        Ok(())
    }

    /// Validate that a crate name is valid.
    fn validate_crate_name(name: &str) -> Result<()> {
        if name.is_empty() {
            bail!("Notebook name cannot be empty");
        }

        if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            bail!("Notebook name '{}' contains invalid characters. Use only alphanumeric, '-', or '_'.", name);
        }

        if name.starts_with(|c: char| c.is_numeric()) {
            bail!("Notebook name '{}' cannot start with a number", name);
        }

        Ok(())
    }

    /// Create a new Cargo.toml with the notebook as a binary.
    fn create_bin_manifest(&self, notebook_name: &str, notebook_path: &Path) -> Result<()> {
        let manifest_path = self.manifest_dir.join("Cargo.toml");

        let venus_dep = self.format_venus_dependency();
        let bin_path = notebook_path.display();

        let content = format!(
            r#"[package]
name = "venus-notebooks"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{notebook_name}"
path = "{bin_path}"

[dependencies]
{venus_dep}
serde = {{ version = "1", features = ["derive"] }}
"#
        );

        fs::write(&manifest_path, content)
            .context("Failed to write Cargo.toml")?;

        println!("✓ Created Cargo.toml with notebook '{}' as binary", notebook_name);
        Ok(())
    }

    /// Add a bin entry to existing Cargo.toml
    fn add_bin_to_manifest(&self, notebook_name: &str, notebook_path: &Path) -> Result<()> {
        let manifest_path = self.manifest_dir.join("Cargo.toml");
        let content = fs::read_to_string(&manifest_path)?;

        // Check if bin already exists
        if self.bin_exists(&content, notebook_name)? {
            bail!("Binary '{}' already exists in Cargo.toml", notebook_name);
        }

        let bin_path = notebook_path.display();
        let bin_entry = format!(
            "\n[[bin]]\nname = \"{}\"\npath = \"{}\"\n",
            notebook_name, bin_path
        );

        // Find where to insert the bin entry
        // Insert before [dependencies] if it exists, otherwise append
        let new_content = if let Some(pos) = content.find("[dependencies]") {
            let (before, after) = content.split_at(pos);
            format!("{}{}{}", before, bin_entry, after)
        } else {
            format!("{}{}", content, bin_entry)
        };

        fs::write(&manifest_path, new_content)
            .context("Failed to update Cargo.toml")?;

        println!("✓ Added notebook '{}' as binary to Cargo.toml", notebook_name);
        Ok(())
    }

    /// Check if a bin entry already exists
    fn bin_exists(&self, content: &str, name: &str) -> Result<bool> {
        // Simple check: look for [[bin]] followed by name = "..."
        // This is not perfect TOML parsing but sufficient for our use case
        let bin_pattern = format!("name = \"{}\"", name);

        let mut in_bin_section = false;
        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed == "[[bin]]" {
                in_bin_section = true;
            } else if trimmed.starts_with('[') && in_bin_section {
                in_bin_section = false;
            }

            if in_bin_section && trimmed.contains(&bin_pattern) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Create a workspace Cargo.toml
    fn create_workspace_manifest(&self, notebook_name: &str) -> Result<()> {
        let manifest_path = self.manifest_dir.join("Cargo.toml");

        let content = format!(
            r#"[workspace]
members = ["{notebook_name}"]
resolver = "2"
"#
        );

        fs::write(&manifest_path, content)
            .context("Failed to write Cargo.toml")?;

        // Create member directory with its own Cargo.toml
        self.create_workspace_member(notebook_name)?;

        println!("✓ Created workspace with member '{}'", notebook_name);
        Ok(())
    }

    /// Add a member to existing workspace
    fn add_workspace_member(&self, notebook_name: &str) -> Result<()> {
        let manifest_path = self.manifest_dir.join("Cargo.toml");
        let content = fs::read_to_string(&manifest_path)?;

        // Check if member already exists
        if content.contains(&format!("\"{}\"", notebook_name)) {
            bail!("Workspace member '{}' already exists in Cargo.toml", notebook_name);
        }

        // Find the members array and add the new member
        let new_content = if let Some(start) = content.find("members = [") {
            let after_bracket = start + "members = [".len();
            let before = &content[..after_bracket];
            let after = &content[after_bracket..];

            // Check if this is an empty array
            if after.trim_start().starts_with(']') {
                // Empty array - just add the member
                format!("{}\"{}\"]{}", before, notebook_name, &after[1..])
            } else {
                // Non-empty - add with comma
                format!("{}\"{}\", {}", before, notebook_name, after)
            }
        } else {
            bail!("Could not find 'members' array in workspace Cargo.toml");
        };

        fs::write(&manifest_path, new_content)
            .context("Failed to update Cargo.toml")?;

        // Create the member directory
        self.create_workspace_member(notebook_name)?;

        println!("✓ Added workspace member '{}'", notebook_name);
        Ok(())
    }

    /// Create a workspace member directory with Cargo.toml
    fn create_workspace_member(&self, notebook_name: &str) -> Result<()> {
        let member_dir = self.manifest_dir.join(notebook_name);
        fs::create_dir_all(&member_dir)
            .context("Failed to create workspace member directory")?;

        let member_manifest = member_dir.join("Cargo.toml");
        let venus_dep = self.format_venus_dependency();

        let content = format!(
            r#"[package]
name = "{notebook_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{notebook_name}"
path = "{notebook_name}.rs"

[dependencies]
{venus_dep}
serde = {{ version = "1", features = ["derive"] }}
"#
        );

        fs::write(&member_manifest, content)
            .context("Failed to write member Cargo.toml")?;

        Ok(())
    }

    /// Format the venus dependency line based on how venus was found
    fn format_venus_dependency(&self) -> String {
        let path_str = self.venus_path.display().to_string();

        // If it's just "venus", use the registry version
        if path_str == "venus" {
            r#"venus = "0.1""#.to_string()
        } else {
            // Path dependency
            format!(r#"venus = {{ path = "{}" }}"#, path_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_crate_name() {
        assert!(CargoManager::validate_crate_name("my_notebook").is_ok());
        assert!(CargoManager::validate_crate_name("notebook-1").is_ok());
        assert!(CargoManager::validate_crate_name("notebook_name_123").is_ok());

        assert!(CargoManager::validate_crate_name("").is_err());
        assert!(CargoManager::validate_crate_name("123start").is_err());
        assert!(CargoManager::validate_crate_name("my notebook").is_err());
        assert!(CargoManager::validate_crate_name("my/notebook").is_err());
    }

    #[test]
    fn test_bin_exists() {
        let manager = CargoManager {
            manifest_dir: PathBuf::from("/tmp"),
            venus_path: PathBuf::from("venus"),
        };

        let content = r#"
[package]
name = "test"

[[bin]]
name = "notebook1"
path = "notebook1.rs"

[[bin]]
name = "notebook2"
path = "notebook2.rs"
"#;

        assert!(manager.bin_exists(content, "notebook1").unwrap());
        assert!(manager.bin_exists(content, "notebook2").unwrap());
        assert!(!manager.bin_exists(content, "notebook3").unwrap());
    }
}
