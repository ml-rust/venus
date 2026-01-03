# venus-core

[![Crates.io](https://img.shields.io/crates/v/venus-core.svg)](https://crates.io/crates/venus-core)
[![Documentation](https://docs.rs/venus-core/badge.svg)](https://docs.rs/venus-core)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Core engine for Venus reactive notebook environment.

## Overview

This crate provides the internal engine that powers Venus notebooks:

- **Graph engine** - Dependency analysis and reactive execution using petgraph
- **Compiler** - Cranelift JIT compilation for fast development iteration
- **State management** - Serialization and schema evolution with rkyv
- **Execution** - Cell execution and hot-reload with process isolation
- **Incremental computation** - Powered by salsa for efficient re-execution

This is an internal implementation crate. Most users should use the `venus` or `venus-cli` crates instead.

## Architecture

- `graph/` - Dependency graph construction and analysis
- `compile/` - Cranelift and LLVM compilation backends
- `state/` - Serialization and state management
- `execute/` - Cell execution engine with hot-reload
- `parser/` - Rust AST parsing for cell extraction

## Documentation

For complete documentation, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [Technical Design Document](https://github.com/ml-rust/venus/blob/main/TDD.md)
- [API Documentation](https://docs.rs/venus-core)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
