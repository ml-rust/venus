# Installation Guide

This guide covers installing Venus on your system.

## Prerequisites

Venus requires a Rust toolchain. The codebase uses Edition 2024, which requires a recent version of Rust.

**Minimum Requirements:**
- Rust 1.85+ (for Edition 2024 support)
- Cargo (comes with Rust)
- Operating System: Linux, macOS, or Windows

## Install Rust

If you don't have Rust installed, get it from [rustup.rs](https://rustup.rs):

**Linux / macOS:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Windows:**
Download and run [rustup-init.exe](https://win.rustup.rs)

After installation, restart your terminal and verify:
```bash
rustc --version
cargo --version
```

## Install Venus

```bash
cargo install venus
```

This compiles and installs the `venus` and `venus-worker` binaries to `~/.cargo/bin/`.

Verify installation:
```bash
venus --version
```

## ⚠️ Security Note

**Venus is designed for local development, testing, and learning environments.**

Venus executes arbitrary Rust code with full system access - no sandboxing or isolation. Only run notebooks you trust.

- ✅ Safe for: Your own code on your own machine
- ❌ Not safe for: Production, shared servers, untrusted code

For deployment and cloud usage, see [SECURITY.md](../SECURITY.md) and [Deployment Guide](deployment.md).

## Optional: Cranelift Backend

Venus can use Cranelift for faster compilation during development. **This is completely optional** - Venus works fine without it using the standard LLVM backend.

Cranelift provides significantly faster compile times (<1 second vs several seconds), making the interactive notebook experience smoother.

To install Cranelift:
```bash
rustup component add rustc-codegen-cranelift-preview
```

Venus automatically detects and uses Cranelift if available. No configuration needed.

## Next Steps

Create and run your first notebook:
```bash
venus new hello
venus run hello.rs
```

Or start the interactive web server:
```bash
venus serve hello.rs
```

Then open http://localhost:8080 in your browser.

## Troubleshooting

### "rustc not found"

Restart your terminal after installing Rust, or manually add Cargo's bin directory to PATH:

```bash
# Linux/macOS
export PATH="$HOME/.cargo/bin:$PATH"
```

### "edition2024" error

Update Rust to the latest version:
```bash
rustup update
```

### Compilation fails

If `cargo install venus` fails:
1. Update Rust: `rustup update`
2. Check you have enough memory (at least 2GB RAM)
3. Try with fewer parallel jobs: `cargo install venus -j 2`

## Platform-Specific Notes

### Windows

Venus works on both MSVC and GNU toolchains. MSVC is recommended:
- Install Visual Studio Build Tools or Visual Studio with C++ tools
- Then install Rust via rustup-init.exe

### Linux

No special requirements. Works on all major distributions.

### macOS

Works on both Intel and Apple Silicon (M1/M2/M3) natively.

## Updating

To update Venus to the latest version:
```bash
cargo install venus --force
```

## Further Documentation

- [Getting Started](getting-started.md) - Create your first notebook
- [CLI Reference](cli.md) - All Venus commands
- [Troubleshooting](troubleshooting.md) - Common issues
