# Widgets

Widgets add interactivity to your notebooks. When a widget value changes, the cell automatically re-executes.

## Slider

Create numeric sliders:

```rust
#[venus::cell]
pub fn analysis() -> f64 {
    let threshold = venus::input_slider("threshold", 0.0, 100.0, 50.0);
    // Use threshold value...
    threshold * 2.0
}
```

### Variants

```rust
// Basic slider
input_slider(id, min, max, default)

// With custom step
input_slider_with_step(id, min, max, step, default)

// With custom label
input_slider_labeled(id, label, min, max, step, default)
```

## Text Input

Create text fields:

```rust
#[venus::cell]
pub fn greeting() -> String {
    let name = venus::input_text("name", "Enter your name");
    format!("Hello, {}!", name)
}
```

### Variants

```rust
// Basic text input
input_text(id, placeholder)

// With default value
input_text_with_default(id, placeholder, default)

// With custom label
input_text_labeled(id, label, placeholder, default)
```

## Select (Dropdown)

Create dropdown menus:

```rust
#[venus::cell]
pub fn mode() -> String {
    let selected = venus::input_select("mode", &["Fast", "Normal", "Slow"], 1);
    format!("Mode: {}", selected)
}
```

### Variants

```rust
// Basic select
input_select(id, options, default_index)

// With custom label
input_select_labeled(id, label, options, default_index)
```

## Checkbox

Create boolean toggles:

```rust
#[venus::cell]
pub fn feature() -> String {
    let enabled = venus::input_checkbox("enabled", true);
    if enabled {
        "Feature enabled".to_string()
    } else {
        "Feature disabled".to_string()
    }
}
```

### Variants

```rust
// Basic checkbox
input_checkbox(id, default)

// With custom label
input_checkbox_labeled(id, label, default)
```

## Widget IDs

Each widget needs a unique ID within its cell:

```rust
#[venus::cell]
pub fn controls() -> Config {
    let speed = venus::input_slider("speed", 0.0, 100.0, 50.0);
    let quality = venus::input_slider("quality", 1.0, 10.0, 5.0);
    // IDs "speed" and "quality" are unique within this cell
    Config { speed, quality }
}
```

## Value Persistence

Widget values persist across:
- Cell re-execution
- Notebook reload
- Server restart (if saved)

Values are stored by widget ID, so changing the ID resets the value.

## Example

```rust
use venus::prelude::*;

#[venus::cell]
pub fn interactive() -> String {
    let speed = venus::input_slider("speed", 0.0, 100.0, 50.0);
    let name = venus::input_text("name", "Enter name");
    let mode = venus::input_select("mode", &["Auto", "Manual"], 0);
    let debug = venus::input_checkbox("debug", false);

    format!(
        "Speed: {:.1}, Name: {}, Mode: {}, Debug: {}",
        speed, name, mode, debug
    )
}
```
