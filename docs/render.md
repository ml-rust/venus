# Render Trait

The `Render` trait controls how cell outputs are displayed. Venus provides default implementations and allows custom rendering.

## Default Rendering

By default, outputs use `Debug` formatting:

```rust
#[venus::cell]
pub fn numbers() -> Vec<i32> {
    vec![1, 2, 3, 4, 5]
}
// Output: [1, 2, 3, 4, 5]
```

## Built-in Implementations

Venus provides `Render` for common types:

| Type | Rendering |
|------|-----------|
| `String` | Plain text |
| `i32`, `i64`, `f32`, `f64` | Formatted number |
| `bool` | "true" / "false" |
| `Vec<T>` | Debug list |
| `Option<T>` | "Some(x)" / "None" |
| `serde_json::Value` | Pretty JSON |

## Custom Rendering

Implement `Render` for custom types:

```rust
use venus::Render;

pub struct Report {
    pub title: String,
    pub data: Vec<f64>,
}

impl Render for Report {
    fn render_text(&self) -> String {
        format!("{}: {:?}", self.title, self.data)
    }

    fn render_html(&self) -> Option<String> {
        Some(format!(
            "<div class='report'>
                <h2>{}</h2>
                <pre>{:?}</pre>
            </div>",
            self.title, self.data
        ))
    }
}
```

## Render Methods

The trait provides multiple output formats:

```rust
pub trait Render {
    /// Plain text representation
    fn render_text(&self) -> String;

    /// HTML representation (optional)
    fn render_html(&self) -> Option<String> { None }

    /// Image bytes (optional, PNG/SVG)
    fn render_image(&self) -> Option<Vec<u8>> { None }

    /// Structured data (optional, JSON)
    fn render_data(&self) -> Option<serde_json::Value> { None }
}
```

## JSON Wrapper

Use `Json<T>` for pretty-printed JSON:

```rust
use venus::Json;

#[venus::cell]
pub fn config() -> Json<Config> {
    Json(Config {
        name: "Analysis".to_string(),
        threshold: 0.5,
    })
}
```

## Feature Flags

### polars

Enable DataFrame rendering:

```toml
[dependencies]
venus = { version = "0.1", features = ["polars"] }
```

```rust
use polars::prelude::*;

#[venus::cell]
pub fn data() -> DataFrame {
    df! {
        "name" => ["Alice", "Bob"],
        "score" => [95, 87]
    }.unwrap()
}
// Renders as HTML table
```

### image

Enable image rendering:

```toml
[dependencies]
venus = { version = "0.1", features = ["image"] }
```

```rust
use image::DynamicImage;

#[venus::cell]
pub fn chart() -> DynamicImage {
    // Create or load image
    DynamicImage::new_rgb8(100, 100)
}
// Renders as PNG in output
```

## Display Priority

Venus uses outputs in this order:
1. `render_html()` - Rich web display
2. `render_image()` - Visual content
3. `render_text()` - Fallback text

The web UI automatically selects the best available format.
