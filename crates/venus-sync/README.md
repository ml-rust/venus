# venus-sync

[![Crates.io](https://img.shields.io/crates/v/venus-sync.svg)](https://crates.io/crates/venus-sync)
[![Documentation](https://docs.rs/venus-sync/badge.svg)](https://docs.rs/venus-sync)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Sync engine for Venus - converts `.rs` notebooks to `.ipynb` format.

## Overview

This crate handles bidirectional conversion between Venus's native `.rs` notebook format and Jupyter's `.ipynb` format for GitHub preview and compatibility.

## Features

- Convert `.rs` notebooks to `.ipynb` for GitHub rendering
- Preserve cell outputs and metadata
- Markdown cell support
- Image and rich output embedding

This is an internal implementation crate used by the `venus` CLI. Most users don't need to use this directly.

## Documentation

For complete documentation, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [API Documentation](https://docs.rs/venus-sync)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
