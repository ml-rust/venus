# Troubleshooting

**Status**: This is a living document that will be updated based on user feedback.

Venus is currently in active development. As users report issues, this guide will be expanded with solutions to common problems.

---

## Reporting Issues

If you encounter a problem not listed here:

1. **Check existing issues**: [GitHub Issues](https://github.com/ml-rust/venus/issues)
2. **Ask the community**: [Venus Discord](https://discord.gg/EwWqU9Xqpf)
3. **Provide details**:
   - Venus version (`venus --version`)
   - Operating system and version
   - Rust toolchain version (`rustc --version`)
   - Full error message or unexpected behavior
   - Minimal reproduction steps if possible
4. **Create a new issue**: Include the information above

---

## Known Issues

### Platform-Specific

Currently no known platform-specific issues. This section will be updated as issues are reported.

### Build & Compilation

Currently no known build or compilation issues. This section will be updated as issues are reported.

### Runtime & Execution

Currently no known runtime issues. This section will be updated as issues are reported.

---

## General Debugging Tips

### Enable Verbose Logging

Set the `RUST_LOG` environment variable to see detailed logs:

```bash
RUST_LOG=venus=debug venus run your-notebook.rs
```

Or for maximum verbosity:

```bash
RUST_LOG=trace venus run your-notebook.rs
```

### Check Venus Directories

Venus stores build artifacts and state in `.venus/` directory within your notebook directory:

```
your-notebook/
├── .venus/
│   ├── build/          # Compiled cell libraries
│   ├── cache/          # Compilation cache
│   └── state/          # Persisted cell outputs
```

If you encounter state-related issues, you can try cleaning this directory:

```bash
rm -rf .venus/
venus run your-notebook.rs  # Rebuild from scratch
```

### Verify Installation

Ensure all Venus components are correctly installed:

```bash
# Check CLI is installed
venus --version

# Check worker binary is available
which venus-worker  # Unix/macOS
where venus-worker  # Windows
```

---

## Getting Help

- **Documentation**: See other docs in `docs/` directory
- **Examples**: Check `examples/` for working notebooks
- **Community**: Join [Venus Discord](https://discord.gg/EwWqU9Xqpf) for discussions
- **Issues**: Report bugs on [GitHub Issues](https://github.com/ml-rust/venus/issues)
- **Security issues**: Report privately via GitHub Security Advisories

---

**Note**: This guide will be continuously updated as Venus matures and user feedback is collected. Your issue reports help improve this documentation for everyone.
