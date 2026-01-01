//! rust-analyzer management module.
//!
//! Downloads and caches rust-analyzer for LSP support.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// rust-analyzer version to download.
const RUST_ANALYZER_VERSION: &str = "2025-12-29";

/// Get the path to the cached rust-analyzer binary.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("venus")
        .join("bin")
}

/// Get the expected rust-analyzer binary path.
pub fn rust_analyzer_path() -> PathBuf {
    let binary_name = if cfg!(windows) {
        "rust-analyzer.exe"
    } else {
        "rust-analyzer"
    };
    cache_dir().join(binary_name)
}

/// Check if rust-analyzer is available (either cached or in PATH).
pub async fn is_available() -> bool {
    // First check cached version
    let cached = rust_analyzer_path();
    if cached.exists() {
        return true;
    }

    // Fall back to system PATH
    Command::new("rust-analyzer")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Get the rust-analyzer command path (cached or system).
pub async fn get_command_path() -> Option<PathBuf> {
    let cached = rust_analyzer_path();
    if cached.exists() {
        return Some(cached);
    }

    // Check if available in PATH
    if Command::new("rust-analyzer")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return Some(PathBuf::from("rust-analyzer"));
    }

    None
}

/// Download rust-analyzer if not available.
pub async fn ensure_available() -> Result<PathBuf, String> {
    // Check if already available
    if let Some(path) = get_command_path().await {
        tracing::info!("rust-analyzer available at: {}", path.display());
        return Ok(path);
    }

    tracing::info!("rust-analyzer not found, downloading...");
    download().await
}

/// Download rust-analyzer from GitHub releases.
pub async fn download() -> Result<PathBuf, String> {
    let target = get_target_triple();
    let url = format!(
        "https://github.com/rust-lang/rust-analyzer/releases/download/{}/rust-analyzer-{}.gz",
        RUST_ANALYZER_VERSION, target
    );

    tracing::info!("Downloading rust-analyzer from: {}", url);

    // Create cache directory
    let cache = cache_dir();
    tokio::fs::create_dir_all(&cache)
        .await
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;

    // Download with reqwest
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to download rust-analyzer: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download rust-analyzer: HTTP {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Decompress gzip
    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut decompressed = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut decompressed)
        .map_err(|e| format!("Failed to decompress: {}", e))?;

    // Write binary
    let binary_path = rust_analyzer_path();
    let mut file = tokio::fs::File::create(&binary_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;
    file.write_all(&decompressed)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&binary_path)
            .await
            .map_err(|e| format!("Failed to get metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&binary_path, perms)
            .await
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    tracing::info!("rust-analyzer downloaded to: {}", binary_path.display());
    Ok(binary_path)
}

/// Get the target triple for the current platform.
fn get_target_triple() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        compile_error!("Unsupported platform for rust-analyzer download")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_dir() {
        let dir = cache_dir();
        assert!(dir.ends_with("venus/bin") || dir.ends_with("venus\\bin"));
    }

    #[test]
    fn test_target_triple() {
        let triple = get_target_triple();
        assert!(!triple.is_empty());
    }
}
