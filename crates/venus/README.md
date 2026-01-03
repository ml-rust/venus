# venus

[![Crates.io](https://img.shields.io/crates/v/venus.svg)](https://crates.io/crates/venus)
[![Documentation](https://docs.rs/venus/badge.svg)](https://docs.rs/venus)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Main crate for Venus - a reactive notebook environment for Rust.

## Overview

This crate provides the core `#[venus::cell]` macro and the `Render` trait for defining notebook cells and custom output rendering.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
venus = "x.x"
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

- `polars` - Enable DataFrame rendering support
- `image` - Enable image rendering support
- `full` - Enable all optional features

## Documentation

For complete documentation and examples, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [Getting Started Guide](https://github.com/ml-rust/venus/blob/main/docs/getting-started.md)
- [API Documentation](https://docs.rs/venus)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
