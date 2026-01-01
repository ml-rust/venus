//! Serve command implementation for Venus CLI.
//!
//! Starts an interactive WebSocket server for the notebook.

use std::path::Path;

use venus_server::ServerConfig;

use crate::colors;

/// Start the interactive notebook server.
pub async fn execute(notebook_path: &str, port: u16) -> anyhow::Result<()> {
    let path = Path::new(notebook_path);
    if !path.exists() {
        anyhow::bail!("Notebook not found: {}", notebook_path);
    }

    println!(
        "\n{}Venus Server{} - Interactive Notebook",
        colors::BOLD,
        colors::RESET
    );
    println!("{}", "─".repeat(50));

    println!(
        "{}  ◆ Notebook:{} {}",
        colors::CYAN,
        colors::RESET,
        path.display()
    );

    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port,
        open_browser: false,
    };

    println!(
        "{}  ◆ Server:{} http://{}:{}",
        colors::CYAN,
        colors::RESET,
        config.host,
        config.port
    );
    println!(
        "{}  ◆ WebSocket:{} ws://{}:{}/ws",
        colors::CYAN,
        colors::RESET,
        config.host,
        config.port
    );
    println!("{}", "─".repeat(50));
    println!("{}Press Ctrl+C to stop{}", colors::GREEN, colors::RESET);
    println!();

    venus_server::serve(path, config).await?;

    Ok(())
}
