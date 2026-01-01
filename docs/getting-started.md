# Getting Started

This guide will help you install Venus and create your first notebook.

## Installation

Install Venus using Cargo:

```bash
cargo install venus-cli
```

## Creating Your First Notebook

Create a new notebook:

```bash
venus new hello
```

This creates `hello.rs` with a template:

```rust
use venus::prelude::*;

/// Configuration
#[venus::cell]
pub fn config() -> String {
    "Hello, Venus!".to_string()
}

/// Greeting
#[venus::cell]
pub fn greeting(config: &String) -> String {
    format!("{} Welcome to reactive notebooks.", config)
}
```

## Running the Notebook

### Headless Execution

Run all cells and see output in the terminal:

```bash
venus run hello.rs
```

### Interactive Mode

Start the web server:

```bash
venus serve hello.rs
```

Open `http://localhost:8080` in your browser.

## Understanding the Output

When you run a notebook, Venus:

1. Parses all `#[venus::cell]` functions
2. Builds a dependency graph from function parameters
3. Compiles cells using Cranelift JIT
4. Executes cells in topological order
5. Displays formatted output

## Next Steps

- [Cells](cells.md) - Learn about cell syntax and dependencies
- [Widgets](widgets.md) - Add interactive inputs
- [CLI Reference](cli.md) - Explore all commands
- [Render Trait](render.md) - Customize output formatting
