# Venus

![Venus](docs/images/venus.png)

A reactive notebook environment for Rust.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

## What is Venus?

Venus lets you write Rust notebooks as regular `.rs` files with full IDE support. Cells are functions marked with `#[venus::cell]`, and dependencies between cells are automatically inferred from function parameters.

![Venus Web UI](docs/images/screenshot.png)

## Features

- **Interactive web UI** - Monaco editor with syntax highlighting, cell outputs, and execution status
- **Native Rust files** - Write notebooks as `.rs` files with full rust-analyzer support
- **Reactive execution** - Cells automatically re-run when dependencies change
- **Fast compilation** - Cranelift JIT backend for sub-second feedback
- **Hot reload** - Edit code and see results instantly without losing state
- **Interactive widgets** - Sliders, text inputs, dropdowns, and checkboxes
- **Rich output** - Render HTML, images, tables, and custom formats
- **Jupyter export** - Generate `.ipynb` files for GitHub preview

## Quick Start

```bash
# Install Venus
cargo install venus-cli

# Create a new notebook
venus new my_notebook

# Run the notebook
venus run my_notebook.rs

# Start the interactive server
venus serve my_notebook.rs
```

Then open `http://localhost:8080` in your browser.

## Example

```rust
use venus::prelude::*;

/// Configuration for the analysis
#[venus::cell]
pub fn config() -> Config {
    Config { count: 10 }
}

/// Generate squared numbers
#[venus::cell]
pub fn numbers(config: &Config) -> Vec<i32> {
    (1..=config.count).map(|i| i * i).collect()
}

/// Calculate the sum
#[venus::cell]
pub fn total(numbers: &Vec<i32>) -> i32 {
    numbers.iter().sum()
}
```

## CLI Commands

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

See the [docs](docs/) directory for detailed documentation:

- [Getting Started](docs/getting-started.md)
- [Cells](docs/cells.md)
- [Widgets](docs/widgets.md)
- [CLI Reference](docs/cli.md)
- [Render Trait](docs/render.md)

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
