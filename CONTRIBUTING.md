# Contributing to Venus

Thank you for your interest in Venus.

## Before You Start

Open an issue before you start anything non-trivial. Examples: a new CLI command, a cell feature, or an API change.
Discuss design on [Discord](https://discord.gg/jBhFk9kHPg).

## Build and Test

Install stable Rust. Install nightly with the Cranelift component to run the same test setup as CI:

```bash
rustup toolchain install nightly
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
```

Run these commands from the repository root. CI runs the same checks on Linux, macOS, and Windows.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo +stable test --workspace
```

Build a single crate with `cargo build -p venus`, `cargo build -p venus-core`, and so on.

## Conventions

- Place unit tests in the same file under `#[cfg(test)] mod tests`.
- Place integration tests in the `tests/` directory of the crate.
- Use `Result<T, venus_core::Error>` in internal code.
- Map compilation errors back to notebook source locations.
- Give every new public API a doc comment.
- Test hot reload changes on Linux, macOS, and Windows.

## Pull Requests

- Include tests for new behavior and for each bug fix.
- Run the three commands in [Build and Test](#build-and-test) before you open the pull request.
- Update the matching page in `docs/` when you change a cell, widget, or CLI behavior.
- Describe the change, the reason for it, and the commands you ran.
