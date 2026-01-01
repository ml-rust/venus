# Cells

Cells are the building blocks of Venus notebooks. Each cell is a function marked with `#[venus::cell]`.

## Basic Syntax

```rust
/// Cell description (shown in UI)
#[venus::cell]
pub fn cell_name() -> ReturnType {
    // Cell body
}
```

## Dependencies

Dependencies are inferred from function parameters:

```rust
#[venus::cell]
pub fn config() -> i32 {
    42
}

#[venus::cell]
pub fn doubled(config: &i32) -> i32 {
    config * 2
}
```

When `config` changes, `doubled` automatically re-runs.

### Parameter Matching

Parameters must match the return type of the dependency:

| Dependency returns | Parameter type |
|-------------------|----------------|
| `i32` | `&i32` |
| `String` | `&String` |
| `Vec<T>` | `&Vec<T>` |
| `CustomType` | `&CustomType` |

## Doc Comments

Doc comments become cell descriptions:

```rust
/// # Configuration
///
/// This cell defines the analysis parameters.
/// Values can be adjusted to tune the results.
#[venus::cell]
pub fn config() -> Config {
    Config::default()
}
```

Markdown formatting is supported in the web UI.

## Custom Types

Define your own types for cell outputs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub title: String,
    pub values: Vec<f64>,
}

#[venus::cell]
pub fn report(data: &Vec<f64>) -> Report {
    Report {
        title: "Analysis".to_string(),
        values: data.clone(),
    }
}
```

Types must derive `Serialize` and `Deserialize` (Venus transforms these to rkyv for efficient serialization).

## Execution Order

Cells execute in topological order based on dependencies:

```
config -> numbers -> doubled
              \-> tripled
                     \-> combined
```

Independent cells at the same level can run in parallel.

## Hot Reload

When you edit a cell:

1. Only that cell and its dependents recompile
2. State from unaffected cells is preserved
3. Compilation uses Cranelift JIT for speed

## Best Practices

- Keep cells focused on a single task
- Use meaningful names that describe the output
- Add doc comments for complex logic
- Prefer immutable references (`&T`) for parameters
