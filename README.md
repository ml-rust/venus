<div align="center">

# venus

<h3>Reactive notebooks for Rust. Notebooks are plain `.rs` files.</h3>

<p>
Cells are functions marked with `#[venus::cell]`. Dependencies between cells come from function parameters, so rust-analyzer works on the notebook source.
</p>

<p>
  <a href="https://docs.rs/venus"><strong>Docs</strong></a>
  ·
  <a href="https://crates.io/crates/venus"><strong>Crate</strong></a>
  ·
  <a href="#cells"><strong>Cells</strong></a>
  ·
  <a href="#cli-commands"><strong>CLI</strong></a>
  ·
  <a href="#quick-start"><strong>Example</strong></a>
  ·
  <a href="CONTRIBUTING.md"><strong>Contributing</strong></a>
</p>

<p>
  <a href="https://discord.gg/jBhFk9kHPg">
    <img src="https://img.shields.io/discord/1453357769720594543?label=Discord&logo=discord&logoColor=white&color=5865F2" alt="Join the Discord">
  </a>
</p>

<p>
  <a href="https://github.com/ml-rust/venus/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/ml-rust/venus/ci.yml?branch=main&label=ci" alt="CI status">
  </a>
  <a href="https://crates.io/crates/venus">
    <img src="https://img.shields.io/crates/v/venus" alt="crates.io">
  </a>
  <a href="https://docs.rs/venus">
    <img src="https://img.shields.io/docsrs/venus" alt="docs.rs">
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/crates/l/venus" alt="License">
  </a>
  <a href="https://github.com/ml-rust/venus/stargazers">
    <img src="https://img.shields.io/github/stars/ml-rust/venus?style=social" alt="GitHub stars">
  </a>
</p>

</div>

Venus is a reactive notebook environment for Rust. It runs notebooks from `.rs` source files and serves them through a web UI. When an upstream cell changes, Venus marks its dependent cells dirty.

## Security

Venus executes arbitrary Rust code with full system access. It provides no sandboxing. Cells can read and write files, open network connections, and spawn processes.

- Use Venus for local development, testing, and learning.
- Do not run Venus on production servers, shared servers, or with untrusted code.
- Cloud providers must isolate Venus in containers or virtual machines.

You are responsible for running Venus in a secure environment. See [SECURITY.md](SECURITY.md) for details.

## Quick Start

Install the `venus` and `venus-worker` binaries:

```bash
cargo install venus
```

Create a notebook, run it, and start the web server:

```bash
venus new hello
venus run hello.rs
venus serve hello.rs
```

`venus new` adds the notebook as a binary to `Cargo.toml`. This enables rust-analyzer support. Open `http://localhost:8080` in your browser to use the web UI.

### Example

```rust
use venus::prelude::*;

#[derive(Debug, Clone)]
pub struct Config {
    pub count: i32,
}

/// Analysis config
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

## Cells

- A cell is a function marked with `#[venus::cell]`.
- A parameter name refers to the cell with that name. In the example, `numbers` reads the `config` cell.
- The return type is the cell output that other cells read.
- Each `///` doc comment becomes the cell description in the UI.

## Features

- **Web UI:** Monaco editor, cell outputs, and execution status.
- **Native Rust files:** notebooks are `.rs` files with full rust-analyzer support.
- **Reactive dependencies:** Venus marks dependent cells dirty when an upstream cell changes.
- **Hot reload:** modified cells re-run and keep state from the other cells.
- **Widgets:** `input_slider`, `input_text`, `input_select`, and `input_checkbox` build interactive inputs.
- **Rich output:** the `Render` trait produces text, HTML, image, and JSON output. The `polars` feature renders DataFrames. The `image` feature renders PNG output.
- **Markdown cells:** notes between code cells in the web UI.
- **Jupyter export:** `venus sync` writes an `.ipynb` file for GitHub preview.
- **Optional Cranelift:** the Cranelift backend speeds debug builds of cells. Without it, Venus uses the LLVM backend.

See [docs/widgets.md](docs/widgets.md) and [docs/render.md](docs/render.md) for details.

### Optional: Cranelift backend

Install the Cranelift component on nightly:

```bash
rustup toolchain install nightly
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
```

Venus detects the component and uses it when available. No config is needed.

## CLI Commands

| Command                   | Description                                   | Flags                                        |
| ------------------------- | --------------------------------------------- | -------------------------------------------- |
| `venus run <notebook>`    | Run the notebook headlessly               | `--cell <name>`, `--release`                 |
| `venus serve <path>`      | Start the interactive web server              | `-p, --port <port>` (default `8080`)         |
| `venus sync <notebook>`   | Generate an `.ipynb` file                     | `-w, --watch`                                |
| `venus build <notebook>`  | Build a standalone binary                     | `-o, --output <path>`, `--release`           |
| `venus new <name>`        | Create a notebook from a template             | `--workspace`                                |
| `venus export <notebook>` | Export the notebook as a standalone HTML file | `-o, --output <path>`, `--release`, `--dark` |
| `venus watch <notebook>`  | Run the notebook on file changes              | `--cell <name>`, `--release`, `--clear`      |

Pass `-v` or `--verbose` with any subcommand for verbose logging. The `--release` flag uses the LLVM backend with optimizations. `venus new --workspace` creates the notebook as a workspace member in a separate directory.

## Documentation

- [Installation](docs/installation.md): platform setup and the optional Cranelift backend
- [Getting Started](docs/getting-started.md): create your first notebook
- [How It Works](docs/how-it-works.md): execution model and architecture
- [Cells](docs/cells.md): cell syntax and dependencies
- [Widgets](docs/widgets.md): interactive inputs
- [Render Trait](docs/render.md): custom output formatting
- [CLI Reference](docs/cli.md): command-line flags
- [API Reference](docs/api.md): build custom frontends
- [Deployment](docs/deployment.md): deployment guidance
- [FAQ](docs/FAQ.md) and [Troubleshooting](docs/troubleshooting.md)
- [API Stability](STABILITY.md): versioning and breaking change policy

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for conventions and checks.

Questions and design discussion happen on [Discord](https://discord.gg/jBhFk9kHPg).

## License

Licensed under Apache-2.0. See [LICENSE](LICENSE).
