# venus-macros

[![Crates.io](https://img.shields.io/crates/v/venus-macros.svg)](https://crates.io/crates/venus-macros)
[![Documentation](https://docs.rs/venus-macros/badge.svg)](https://docs.rs/venus-macros)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

Procedural macros for Venus reactive notebook environment.

## Overview

This crate provides the `#[venus::cell]` procedural macro that marks functions as notebook cells. It's re-exported by the `venus` crate, so you typically don't need to depend on this directly.

## Usage

This crate is automatically included when you use the `venus` crate:

```rust
use venus::prelude::*;

#[venus::cell]
pub fn my_cell() -> i32 {
    42
}
```

## Documentation

For complete documentation, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [API Documentation](https://docs.rs/venus-macros)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
