//! Toolchain management for Venus compilation.
//!
//! Manages the nightly Rust toolchain with Cranelift codegen backend.

use std::path::PathBuf;
use std::process::Command;

use crate::error::{Error, Result};

/// Manages the Rust toolchain for Venus compilation.
#[derive(Clone)]
pub struct ToolchainManager {
    /// Path to rustup (if available)
    rustup_path: Option<PathBuf>,

    /// Path to rustc
    rustc_path: PathBuf,

    /// Whether Cranelift is available
    cranelift_available: bool,

    /// Toolchain version string
    version: String,
}

impl ToolchainManager {
    /// Create a new toolchain manager, detecting available tools.
    pub fn new() -> Result<Self> {
        let rustup_path = Self::find_rustup();
        let rustc_path = Self::find_rustc()?;
        let version = Self::get_rustc_version(&rustc_path)?;
        let cranelift_available = Self::check_cranelift_available(&rustc_path);

        Ok(Self {
            rustup_path,
            rustc_path,
            cranelift_available,
            version,
        })
    }

    /// Check if Cranelift backend is available.
    pub fn has_cranelift(&self) -> bool {
        self.cranelift_available
    }

    /// Get the rustc path.
    pub fn rustc_path(&self) -> &PathBuf {
        &self.rustc_path
    }

    /// Get the toolchain version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get rustc flags for Cranelift compilation.
    pub fn cranelift_flags(&self) -> Vec<String> {
        if self.cranelift_available {
            vec!["-Zcodegen-backend=cranelift".to_string()]
        } else {
            Vec::new()
        }
    }

    /// Get rustc flags for LLVM compilation.
    pub fn llvm_flags(&self) -> Vec<String> {
        // Default LLVM backend, no special flags needed
        Vec::new()
    }

    /// Ensure the Cranelift component is installed.
    pub fn ensure_cranelift(&mut self) -> Result<()> {
        if self.cranelift_available {
            return Ok(());
        }

        let Some(rustup) = &self.rustup_path else {
            return Err(Error::Compilation {
                cell_id: None,
                message: "rustup not found, cannot install Cranelift component".to_string(),
            });
        };

        tracing::info!("Installing rustc-codegen-cranelift-preview component...");

        let output = Command::new(rustup)
            .args(["component", "add", "rustc-codegen-cranelift-preview"])
            .output()
            .map_err(|e| Error::Compilation {
                cell_id: None,
                message: format!("Failed to run rustup: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Compilation {
                cell_id: None,
                message: format!("Failed to install Cranelift: {}", stderr),
            });
        }

        // Re-check availability
        self.cranelift_available = Self::check_cranelift_available(&self.rustc_path);

        if !self.cranelift_available {
            return Err(Error::Compilation {
                cell_id: None,
                message: "Cranelift component installed but not available".to_string(),
            });
        }

        tracing::info!("Cranelift backend installed successfully");
        Ok(())
    }

    /// Get the sysroot path for linking.
    pub fn sysroot(&self) -> Result<PathBuf> {
        let output = Command::new(&self.rustc_path)
            .args(["--print", "sysroot"])
            .output()
            .map_err(|e| Error::Compilation {
                cell_id: None,
                message: format!("Failed to get sysroot: {}", e),
            })?;

        if !output.status.success() {
            return Err(Error::Compilation {
                cell_id: None,
                message: "Failed to get sysroot".to_string(),
            });
        }

        let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PathBuf::from(sysroot))
    }

    /// Get the target libdir for linking.
    pub fn target_libdir(&self) -> Result<PathBuf> {
        let output = Command::new(&self.rustc_path)
            .args(["--print", "target-libdir"])
            .output()
            .map_err(|e| Error::Compilation {
                cell_id: None,
                message: format!("Failed to get target-libdir: {}", e),
            })?;

        if !output.status.success() {
            return Err(Error::Compilation {
                cell_id: None,
                message: "Failed to get target-libdir".to_string(),
            });
        }

        let libdir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PathBuf::from(libdir))
    }

    /// Find rustup in PATH.
    fn find_rustup() -> Option<PathBuf> {
        which::which("rustup").ok()
    }

    /// Find rustc in PATH.
    fn find_rustc() -> Result<PathBuf> {
        which::which("rustc").map_err(|_| Error::Compilation {
            cell_id: None,
            message: "rustc not found in PATH".to_string(),
        })
    }

    /// Get rustc version string.
    fn get_rustc_version(rustc: &PathBuf) -> Result<String> {
        let output = Command::new(rustc)
            .args(["--version"])
            .output()
            .map_err(|e| Error::Compilation {
                cell_id: None,
                message: format!("Failed to run rustc: {}", e),
            })?;

        if !output.status.success() {
            return Err(Error::Compilation {
                cell_id: None,
                message: "Failed to get rustc version".to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if Cranelift backend is available.
    fn check_cranelift_available(rustc: &PathBuf) -> bool {
        // Try to use Cranelift backend on a simple test
        let output = Command::new(rustc)
            .args(["-Zcodegen-backend=cranelift", "--print", "crate-name", "-"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        output.map(|s| s.success()).unwrap_or(false)
    }
}

impl Default for ToolchainManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize toolchain manager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolchain_detection() {
        let manager = ToolchainManager::new();
        assert!(manager.is_ok(), "Should detect toolchain");

        let manager = manager.unwrap();
        assert!(!manager.version().is_empty());
    }

    #[test]
    fn test_cranelift_flags() {
        let manager = ToolchainManager::new().unwrap();

        if manager.has_cranelift() {
            let flags = manager.cranelift_flags();
            assert!(flags.contains(&"-Zcodegen-backend=cranelift".to_string()));
        }
    }
}
