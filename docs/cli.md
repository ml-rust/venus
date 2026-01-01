# CLI Reference

Venus provides a command-line interface for working with notebooks.

## Commands

### venus run

Execute a notebook headlessly.

```bash
venus run notebook.rs
venus run notebook.rs --cell specific_cell
venus run notebook.rs --release  # Use LLVM for optimized builds
```

**Options:**
- `--cell <name>` - Run only a specific cell and its dependencies
- `--release` - Use LLVM backend for optimized compilation

### venus serve

Start the interactive web server.

```bash
venus serve notebook.rs
venus serve notebook.rs --port 3000
```

**Options:**
- `--port <port>` - Server port (default: 8080)

Open `http://localhost:8080` to access the web UI.

### venus sync

Generate a Jupyter notebook (`.ipynb`) file.

```bash
venus sync notebook.rs
venus sync notebook.rs --watch
```

**Options:**
- `--watch` - Watch for changes and auto-sync

The generated `.ipynb` renders on GitHub for easy sharing.

### venus build

Build the notebook as a standalone binary.

```bash
venus build notebook.rs
venus build notebook.rs -o myapp
venus build notebook.rs --release
```

**Options:**
- `-o, --output <path>` - Output binary path
- `--release` - Build with optimizations

### venus new

Create a new notebook from template.

```bash
venus new mynotebook
```

Creates `mynotebook.rs` with a starter template.

### venus export

Export notebook to standalone HTML.

```bash
venus export notebook.rs
venus export notebook.rs -o report.html
```

**Options:**
- `-o, --output <path>` - Output HTML path (default: `<notebook>.html`)

The HTML includes all cell outputs and can be viewed offline.

### venus watch

Watch notebook and auto-run on changes.

```bash
venus watch notebook.rs
venus watch notebook.rs --clear
```

**Options:**
- `--clear` - Clear screen before each run

## Global Options

All commands support:
- `--help` - Show help information
- `--version` - Show version

## Examples

```bash
# Quick development cycle
venus serve myanalysis.rs

# Generate GitHub-viewable notebook
venus sync myanalysis.rs

# Create shareable report
venus export myanalysis.rs -o report.html

# Continuous testing
venus watch tests.rs --clear
```
