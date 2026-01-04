//! Venus CLI - Reactive notebook environment for Rust.

mod build;
mod cargo_manager;
mod colors;
mod executor;
mod export;
mod output;
mod run;
mod serve;
mod sync;
mod watch;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "venus")]
#[command(about = "Reactive notebook environment for Rust")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a notebook headlessly
    Run {
        /// Path to the notebook (.rs file)
        notebook: String,

        /// Run only a specific cell
        #[arg(long)]
        cell: Option<String>,

        /// Use release mode (LLVM backend, optimized)
        #[arg(long)]
        release: bool,
    },

    /// Start the interactive notebook server
    Serve {
        /// Path to the notebook or directory
        path: String,

        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },

    /// Sync .rs notebook to .ipynb format
    Sync {
        /// Path to the notebook (.rs file)
        notebook: String,

        /// Watch for changes
        #[arg(short, long)]
        watch: bool,
    },

    /// Build notebook as standalone binary
    Build {
        /// Path to the notebook (.rs file)
        notebook: String,

        /// Output path
        #[arg(short, long)]
        output: Option<String>,

        /// Build with optimizations
        #[arg(long)]
        release: bool,
    },

    /// Create a new notebook from template
    New {
        /// Name of the notebook (without .rs extension)
        name: String,

        /// Create as workspace member in separate directory (default: add as binary to Cargo.toml)
        #[arg(long)]
        workspace: bool,
    },

    /// Export notebook as standalone HTML file
    Export {
        /// Path to the notebook (.rs file)
        notebook: String,

        /// Output path for HTML file
        #[arg(short, long)]
        output: Option<String>,

        /// Use release mode (LLVM backend, optimized)
        #[arg(long)]
        release: bool,

        /// Include dark theme (default: true)
        #[arg(long, default_value = "true")]
        dark: bool,
    },

    /// Watch notebook and auto-run on changes
    Watch {
        /// Path to the notebook (.rs file)
        notebook: String,

        /// Run only a specific cell (and its dependencies)
        #[arg(long)]
        cell: Option<String>,

        /// Use release mode (LLVM backend, optimized)
        #[arg(long)]
        release: bool,

        /// Clear screen before each run
        #[arg(long, default_value = "true")]
        clear: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::DEBUG.into())
    } else {
        tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::WARN.into())
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Helper to format venus-core errors with recovery hints
    let format_error = |err: anyhow::Error| -> anyhow::Error {
        if let Some(venus_err) = err.downcast_ref::<venus_core::Error>() {
            anyhow::anyhow!("{}", venus_err.with_hint())
        } else {
            err
        }
    };

    match cli.command {
        Commands::Run {
            notebook,
            cell,
            release,
        } => run::execute(&notebook, cell.as_deref(), release).map_err(format_error)?,

        Commands::Serve { path, port } => {
            serve::execute(&path, port).await.map_err(format_error)?;
        }

        Commands::Sync { notebook, watch } => {
            sync::execute(&notebook, watch).map_err(format_error)?;
        }

        Commands::Build {
            notebook,
            output,
            release,
        } => {
            build::execute(&notebook, output.as_deref(), release).map_err(format_error)?;
        }

        Commands::New { name, workspace } => {
            create_new_notebook(&name, workspace).map_err(format_error)?;
        }

        Commands::Export {
            notebook,
            output,
            release,
            dark,
        } => {
            export::execute(&notebook, output.as_deref(), release, dark).map_err(format_error)?;
        }

        Commands::Watch {
            notebook,
            cell,
            release,
            clear,
        } => {
            watch::execute(&notebook, cell.as_deref(), release, clear).await.map_err(format_error)?;
        }
    }

    Ok(())
}

/// Create a new notebook from template.
fn create_new_notebook(name: &str, workspace: bool) -> anyhow::Result<()> {
    use std::fs;
    use std::path::{Path, PathBuf};
    use cargo_manager::{CargoManager, IntegrationMode};

    // Determine notebook name and file path
    let (notebook_name, filename, notebook_dir) = if workspace {
        // Workspace mode: create in subdirectory
        let notebook_name = name.trim_end_matches(".rs");
        let notebook_dir = PathBuf::from(notebook_name);
        let filename = format!("{}.rs", notebook_name);
        (notebook_name.to_string(), filename, Some(notebook_dir))
    } else {
        // Binary mode: create in current directory
        let filename = if name.ends_with(".rs") {
            name.to_string()
        } else {
            format!("{}.rs", name)
        };
        let notebook_name = Path::new(&filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        (notebook_name, filename, None)
    };

    // Create directory if workspace mode
    if let Some(ref dir) = notebook_dir {
        if dir.exists() {
            anyhow::bail!("Directory {} already exists", dir.display());
        }
        fs::create_dir_all(dir)?;
    }

    // Determine full path to notebook file
    let notebook_path = if let Some(ref dir) = notebook_dir {
        dir.join(&filename)
    } else {
        PathBuf::from(&filename)
    };

    if notebook_path.exists() {
        anyhow::bail!("File {} already exists", notebook_path.display());
    }

    // Generate notebook template
    let template = format!(
        r#"//! # {name}
//!
//! A Venus reactive notebook.
//!
//! ```cargo
//! [dependencies]
//! serde = {{ version = "1", features = ["derive"] }}
//! ```

use venus::prelude::*;

/// First cell - returns a greeting message.
#[venus::cell]
pub fn greeting() -> String {{
    "Hello from Venus!".to_string()
}}

/// Second cell - processes the greeting.
#[venus::cell]
pub fn process(greeting: &String) -> String {{
    format!("Processed: {{}}", greeting)
}}
"#,
        name = notebook_name
    );

    fs::write(&notebook_path, template)?;
    println!("Created new notebook: {}", notebook_path.display());

    // Update Cargo.toml for LSP support
    // Cargo.toml is ALWAYS created in the current working directory (root)
    // UNLESS a path was specified, then extract the root from that path
    let root_dir = if name.contains('/') || name.contains('\\') {
        // Path was specified, use its parent as root, or current dir if no parent
        match Path::new(name).parent() {
            Some(p) => p.to_path_buf(),
            None => std::env::current_dir()?,
        }
    } else {
        // No path specified, use current directory as root
        std::env::current_dir()?
    };

    let cargo_manager = CargoManager::new(&root_dir)?;

    let mode = if workspace {
        IntegrationMode::WorkspaceMember
    } else {
        IntegrationMode::Binary
    };

    // Get relative path for Cargo.toml
    let relative_path = if workspace {
        PathBuf::from(format!("{}/{}", notebook_name, filename))
    } else {
        PathBuf::from(&filename)
    };

    if let Err(e) = cargo_manager.add_notebook(&notebook_name, &relative_path, mode) {
        eprintln!("Warning: Could not update Cargo.toml: {}", e);
        eprintln!("LSP features may not work. You can manually create a Cargo.toml.");
    }

    Ok(())
}
