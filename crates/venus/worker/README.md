# venus-worker

[![Crates.io](https://img.shields.io/crates/v/venus-worker.svg)](https://crates.io/crates/venus-worker)
[![Documentation](https://docs.rs/venus-worker/badge.svg)](https://docs.rs/venus-worker)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Worker process for Venus cell execution with process isolation.

## Overview

This crate provides the worker process that executes notebook cells in isolation:

- **Process isolation** - Each notebook runs in a separate worker process
- **Dynamic loading** - Hot-reload of compiled cell libraries
- **IPC communication** - Bincode-based communication with the main process
- **State preservation** - Maintains cell state across reloads

This is an internal implementation binary used by `venus-core`. Most users don't need to interact with this directly.

## Documentation

For complete documentation, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [Technical Design Document](https://github.com/ml-rust/venus/blob/main/TDD.md)
- [API Documentation](https://docs.rs/venus-worker)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
