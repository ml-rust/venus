# Frequently Asked Questions

## What is Venus?

Venus is a **reactive notebook environment for Rust**. It lets you write Rust code in cells, and automatically manages dependencies between them. When you run a cell, Venus recompiles only that cell (with smart caching) and marks dependent cells as dirty for you to execute.

Key features:

- **Reactive dependency tracking**: Run a cell, and dependent cells are marked dirty (yellow indicator)
- **Fast iteration**: Cranelift JIT compilation for rapid development
- **Hot-reload**: Code changes reload without losing state
- **Native Rust**: Full rust-analyzer support, no external runtime
- **Source-first**: Uses `.rs` files as source (not `.ipynb`)

## How do I install Venus?

**Requirements:**

- Rust stable toolchain (1.85.0 or later)
- Supported platforms: Linux, macOS, Windows

**Installation:**

```bash
cargo install venus
```

This installs both the `venus` and `venus-worker` binaries with no external dependencies.

## How do I use Venus?

**Create a new notebook:**

```bash
venus new my-notebook
```

This creates `my-notebook.rs` with example cells. Edit it with any editor (VS Code, Neovim, etc.) and get full rust-analyzer support.

**Run interactively:**

```bash
venus serve my-notebook.rs
```

Opens a web UI at `http://localhost:8080` where you can see outputs, interact with widgets, and run cells manually.

**Run headlessly:**

```bash
venus run my-notebook.rs
```

Executes all cells and shows outputs in your terminal.

See [Getting Started](getting-started.md) for a full tutorial.

## What's the difference between Venus and evcxr?

| Feature             | Venus                                        | evcxr                            |
| ------------------- | -------------------------------------------- | -------------------------------- |
| **Type**            | Reactive notebook                            | REPL (Read-Eval-Print Loop)      |
| **Execution model** | Dependency graph with dirty tracking         | Sequential evaluation            |
| **State**           | Preserved across hot-reloads                 | Lost on code changes             |
| **Source format**   | `.rs` files (full LSP support)               | Jupyter `.ipynb` or REPL         |
| **Compilation**     | Cranelift (fast dev) + LLVM (optimized)      | Incremental rustc                |
| **Use case**        | Data exploration, prototyping, visualization | Interactive REPL, Jupyter kernel |

**When to use Venus:**

- You want reactive cells (change upstream, downstream marked dirty)
- You want fast compile-edit-run cycles
- You want to keep your notebook as a regular `.rs` file
- You want hot-reload without losing state

**When to use evcxr:**

- You need a REPL for quick experiments
- You're already using Jupyter and want Rust support
- You prefer sequential evaluation

## Does Venus have a faster REPL cycle than evcxr?

**Yes, for development workflows:**

Venus uses **Cranelift JIT** for development, which compiles Rust to native code much faster than LLVM (used by evcxr and standard rustc). This gives Venus a significant speed advantage during the edit-compile-run cycle.

**Typical compile times** (for a single cell with dependencies):

- **Cranelift (Venus dev mode)**: ~100-500ms
- **LLVM (evcxr/rustc)**: ~2-5 seconds

**Trade-off:**

- Cranelift: Fast compilation, slower runtime performance
- LLVM: Slow compilation, optimized runtime performance

Venus lets you choose:

- `venus serve` → Fast Cranelift for iteration
- `venus run --release` → Optimized LLVM for production

**Hot-reload advantage:**

Venus preserves state across code changes. When you run a modified cell, only that cell recompiles (smart caching checks source hash). Dependent cells stay compiled and only get marked dirty if output changed. evcxr typically requires restarting the kernel or re-evaluating from scratch.

**Proof:**

Create a notebook with 10 cells and change the 5th cell:

- **Venus**: Recompiles only cell 5 in ~100ms (Cranelift), marks cells 6-10 dirty (instant), user runs dirty cells as needed
- **evcxr**: Must re-evaluate cells 1-10, each taking 2-5s (LLVM)

See [Performance Guide](performance.md) for benchmarks and optimization tips.

## Is Venus like Pluto.jl?

**Yes, Venus takes heavy inspiration from Pluto.jl:**

**Similarities:**

- **Reactive execution model**: Cells form a dependency graph
- **Dirty tracking**: Run upstream cells, downstream cells marked dirty (Pluto auto-executes; Venus requires manual run)
- **Source-first**: Regular source files (`.rs` vs `.jl`)
- **No cell execution order**: Dependencies determined by code analysis, not manual ordering

**Differences:**

- **Language**: Rust vs Julia
- **Compilation**: Venus compiles to native code (Cranelift/LLVM), Pluto.jl uses Julia's JIT
- **Type safety**: Venus benefits from Rust's static type system and compile-time checks
- **State serialization**: Venus uses zero-copy serialization (rkyv) for efficient state management

If you like Pluto.jl's reactive model but want to work in Rust, Venus is for you.

For a deep dive into Venus's execution model, see [How It Works](HOW_IT_WORKS.md).

## Why not just use Jupyter with evcxr_jupyter?

**Jupyter with evcxr_jupyter is great, but Venus offers:**

1. **True `.rs` source files**

   - Jupyter stores code in `.ipynb` JSON format
   - Venus uses regular `.rs` files with special comments
   - Full rust-analyzer support (autocomplete, go-to-definition, refactoring)
   - Easy to version control (`.rs` diffs vs JSON diffs)

2. **Reactive dependency tracking**

   - Jupyter cells execute in manual order
   - Venus tracks cell dependencies automatically
   - Run a cell → dependent cells marked dirty (yellow)

3. **Faster iteration**

   - Cranelift JIT (~100ms) vs LLVM (~3s) per cell
   - Hot-reload preserves state across changes
   - No kernel restarts

4. **No external runtime**

   - Jupyter requires Python + Jupyter server
   - Venus is a single Rust binary

5. **Native Rust tooling**
   - Integrates with cargo, clippy, rustfmt
   - Can build standalone binaries from notebooks
   - Export to HTML for sharing

**Use Jupyter when:**

- You're already invested in Jupyter ecosystem
- You need polyglot notebooks (mixing languages)
- You need Jupyter extensions (nbconvert, voila, etc.)

**Use Venus when:**

- You want native Rust development experience
- You want reactive cells (like Pluto.jl)
- You want `.rs` files with full LSP support
- You want fast compile cycles with Cranelift

## Why not just use evcxr_jupyter?

See "Why not just use Jupyter?" above. Additionally:

**evcxr_jupyter** is excellent for bringing Rust to Jupyter, but:

1. **No reactivity**: Cells are sequential, not reactive
2. **Slower compilation**: Uses LLVM (2-5s per cell) vs Cranelift (100ms)
3. **State loss on changes**: Modifying code often requires kernel restart
4. **Jupyter dependency**: Needs Python + Jupyter infrastructure

**Venus complements evcxr:**

- Use **evcxr_jupyter** when you need Jupyter's ecosystem
- Use **Venus** when you want reactive notebooks with fast iteration

Both tools serve different needs. Venus focuses on **native Rust development experience** with **dependency tracking** and **fast compile cycles**.

## Can I use async code in cells?

**Yes, but with limitations:**

Venus cells are synchronous functions. For async code, you need to block on futures:

```rust
use venus::prelude::*;

#[venus::cell]
pub fn fetch_data() -> String {
    // Use tokio::runtime::Handle or futures::executor::block_on
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Your async code here
        reqwest::get("https://api.example.com")
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    })
}
```

**Why not fully async?**

Venus uses synchronous FFI calls for cell execution. Supporting async would require:

- Async runtime per cell (overhead)
- Complex lifetime management across FFI boundary
- Potential runtime conflicts between cells

For most notebook use cases (data processing, visualization), blocking async is sufficient.

## How do I handle panics in cells?

Venus isolates each cell in a separate process, so **panics don't crash the entire notebook**:

```rust
#[venus::cell]
pub fn might_panic() -> i32 {
    panic!("Oops!");  // This only kills this cell's process
}

#[venus::cell]
pub fn safe_cell() -> i32 {
    42  // This cell continues to work
}
```

**Error handling:**

When a cell panics:

1. Venus captures the panic message
2. Shows the error in the UI
3. Marks the cell as failed
4. Dependent cells don't execute (wait for fix)

**Best practices:**

- Use `Result<T, E>` for expected errors
- Add `?` operators for error propagation
- Reserve panics for truly unrecoverable errors

See [Error Handling](cells.md#error-handling) for details.

## What are the performance expectations?

**Compilation times:**

| Mode            | Single Cell | 10 Cells | 100 Cells |
| --------------- | ----------- | -------- | --------- |
| Cranelift (dev) | ~100ms      | ~500ms   | ~2s       |
| LLVM (release)  | ~3s         | ~15s     | ~60s      |

**Execution times:**

Depends on your code. Cranelift generates code ~1.5-3x slower than LLVM, but:

- Most notebook operations are I/O bound (file reading, network)
- Computation-heavy cells benefit from `venus run --release`

**Hot-reload latency:**

- Edit cell → see results: ~200ms (Cranelift)
- Includes: compilation + execution + UI update

**Memory usage:**

- Base: ~50MB (Venus server + compiler)
- Per cell: ~1-5MB (depends on data structures)
- State cache: Zero-copy serialization (rkyv) for efficiency

See [Performance Guide](performance.md) for optimization tips.

## How do I share notebooks?

**Several options:**

1. **Share `.rs` file** (most common)

   - Send the `.rs` file
   - Recipient runs `venus serve notebook.rs`
   - Requires Rust + Venus installation

2. **Export to HTML**

   ```bash
   venus export notebook.rs -o output.html
   ```

   - Self-contained HTML with outputs
   - No Venus installation needed
   - Great for reports/presentations

3. **Build standalone binary**

   ```bash
   venus build notebook.rs --release
   ```

   - Compiles to executable
   - Runs without Venus
   - Good for automation/deployment

4. **Sync to Jupyter**
   ```bash
   venus sync notebook.rs
   ```
   - Creates `notebook.ipynb`
   - View on GitHub, Jupyter, etc.
   - One-way export (editing `.ipynb` won't update `.rs`)

See [Deployment Guide](deployment.md) for details.

## Where can I get help?

- **Documentation**: [https://github.com/ml-rust/venus/tree/main/docs](../README.md)
- **Issues**: Report bugs at [GitHub Issues](https://github.com/ml-rust/venus/issues)
- **Examples**: Check `examples/` directory in the repository
- **Troubleshooting**: See [troubleshooting.md](troubleshooting.md)

## Is Venus production-ready?

Venus is currently in **active development** (0.1.0):

- ✅ Core features are stable and tested
- ✅ APIs are mostly stable (see [STABILITY.md](../STABILITY.md))
- ⚠️ Expect some API changes before 1.0
- ⚠️ Not recommended for critical production systems yet

**Use Venus for:**

- Data exploration and analysis
- Prototyping and experimentation
- Learning Rust interactively
- Research and visualization

**Wait for 1.0 for:**

- Production data pipelines
- Mission-critical systems
- Long-term API stability guarantees

See [STABILITY.md](../STABILITY.md) for versioning policy.
