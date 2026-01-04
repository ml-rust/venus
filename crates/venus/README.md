# venus

[![Crates.io](https://img.shields.io/crates/v/venus.svg)](https://crates.io/crates/venus)
[![Documentation](https://docs.rs/venus/badge.svg)](https://docs.rs/venus)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Main crate for Venus - a reactive notebook environment for Rust.

## Overview

This crate provides:
- The core `#[venus::cell]` macro for defining notebook cells
- The `Render` trait for custom output rendering
- Interactive widgets (sliders, text inputs, checkboxes, dropdowns)
- CLI binaries (`venus` and `venus-worker`) when installed with `cargo install venus`

## Installation

**As a CLI tool** (recommended for most users):

```bash
cargo install venus
```

This installs both the `venus` and `venus-worker` binaries.

**As a library** (for embedding in other projects):

Add this to your `Cargo.toml`:

```toml
[dependencies]
venus = { version = "x.x", default-features = false }
```

## Example

```rust
use venus::prelude::*;

/// A simple cell that returns a number
#[venus::cell]
pub fn number() -> i32 {
    42
}

/// A cell that depends on the previous cell
#[venus::cell]
pub fn doubled(number: &i32) -> i32 {
    number * 2
}
```

## Features

- `cli` (default) - Include CLI binaries and their dependencies
- `polars` - Enable DataFrame rendering support
- `image` - Enable image rendering support
- `full` - Enable all optional features (cli, polars, image)

## Documentation

For complete documentation and examples, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [Getting Started Guide](https://github.com/ml-rust/venus/blob/main/docs/getting-started.md)
- [API Documentation](https://docs.rs/venus)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
