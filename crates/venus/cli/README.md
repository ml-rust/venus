# venus CLI

Command-line interface for Venus reactive notebook environment.

## Overview

This CLI is bundled with the `venus` crate and is **not** published separately to crates.io.

## Installation

```bash
cargo install venus
```

This installs both binaries:
- `venus` - Main CLI tool
- `venus-worker` - Worker process for isolated cell execution

## Quick Start

```bash
# Create a new notebook (generates Cargo.toml for LSP)
venus new my_notebook

# Or create as workspace member
venus new my_notebook --workspace

# Run the notebook headlessly
venus run my_notebook.rs

# Start the interactive web server
venus serve my_notebook.rs
```

Then open `http://localhost:8080` in your browser.

## Commands

| Command | Description |
|---------|-------------|
| `venus run <notebook>` | Execute notebook headlessly |
| `venus serve <notebook>` | Start interactive web server |
| `venus sync <notebook>` | Generate `.ipynb` file |
| `venus build <notebook>` | Build standalone binary |
| `venus new <name>` | Create new notebook |
| `venus export <notebook>` | Export to standalone HTML |
| `venus watch <notebook>` | Auto-run on file changes |

## Documentation

For complete documentation and examples, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [Getting Started Guide](https://github.com/ml-rust/venus/blob/main/docs/getting-started.md)
- [CLI Reference](https://github.com/ml-rust/venus/blob/main/docs/cli.md)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
