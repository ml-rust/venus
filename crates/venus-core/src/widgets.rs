//! Interactive widgets for Venus notebooks.
//!
//! Widgets allow notebook users to create interactive inputs like sliders,
//! text boxes, and dropdowns that trigger cell re-execution when values change.
//!
//! # Example
//!
//! ```rust,ignore
//! use venus::prelude::*;
//!
//! #[venus::cell]
//! pub fn interactive() -> String {
//!     let speed = venus::input_slider("speed", 0.0, 100.0, 50.0);
//!     let name = venus::input_text("name", "Enter your name");
//!     let mode = venus::input_select("mode", &["Fast", "Slow", "Auto"], 0);
//!
//!     format!("Speed: {}, Name: {}, Mode: {}", speed, name, mode)
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

/// Widget definition sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WidgetDef {
    /// Numeric slider widget.
    Slider {
        /// Unique widget ID within the cell.
        id: String,
        /// Human-readable label.
        label: String,
        /// Minimum value.
        min: f64,
        /// Maximum value.
        max: f64,
        /// Step increment.
        step: f64,
        /// Current value.
        value: f64,
    },
    /// Text input widget.
    TextInput {
        /// Unique widget ID within the cell.
        id: String,
        /// Human-readable label.
        label: String,
        /// Placeholder text.
        placeholder: String,
        /// Current value.
        value: String,
    },
    /// Dropdown select widget.
    Select {
        /// Unique widget ID within the cell.
        id: String,
        /// Human-readable label.
        label: String,
        /// Available options.
        options: Vec<String>,
        /// Currently selected index.
        selected: usize,
    },
    /// Checkbox widget.
    Checkbox {
        /// Unique widget ID within the cell.
        id: String,
        /// Human-readable label.
        label: String,
        /// Current value.
        value: bool,
    },
}

impl WidgetDef {
    /// Get the widget ID.
    pub fn id(&self) -> &str {
        match self {
            WidgetDef::Slider { id, .. } => id,
            WidgetDef::TextInput { id, .. } => id,
            WidgetDef::Select { id, .. } => id,
            WidgetDef::Checkbox { id, .. } => id,
        }
    }
}

/// Widget value that can be stored in state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WidgetValue {
    /// Numeric value (for sliders).
    Number(f64),
    /// String value (for text inputs).
    Text(String),
    /// Index value (for selects).
    Index(usize),
    /// Boolean value (for checkboxes).
    Bool(bool),
}

impl WidgetValue {
    /// Get as f64 if it's a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            WidgetValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as String if it's text.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            WidgetValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get as usize if it's an index.
    pub fn as_index(&self) -> Option<usize> {
        match self {
            WidgetValue::Index(i) => Some(*i),
            _ => None,
        }
    }

    /// Get as bool if it's a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            WidgetValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Thread-local widget context for the current cell execution.
///
/// This is set by the executor before calling the cell function,
/// and allows widgets to:
/// 1. Register themselves (so the frontend knows about them)
/// 2. Read their current value (set by user interaction)
#[derive(Debug, Default)]
pub struct WidgetContext {
    /// Registered widgets during this execution.
    pub widgets: Vec<WidgetDef>,
    /// Current widget values (set by user interaction).
    pub values: HashMap<String, WidgetValue>,
}

impl WidgetContext {
    /// Create a new empty widget context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a widget context with pre-set values.
    pub fn with_values(values: HashMap<String, WidgetValue>) -> Self {
        Self {
            widgets: Vec::new(),
            values,
        }
    }

    /// Register a widget and return its current value.
    fn register_slider(&mut self, id: &str, label: &str, min: f64, max: f64, step: f64, default: f64) -> f64 {
        let value = self
            .values
            .get(id)
            .and_then(|v| v.as_f64())
            .unwrap_or(default)
            .clamp(min, max);

        self.widgets.push(WidgetDef::Slider {
            id: id.to_string(),
            label: label.to_string(),
            min,
            max,
            step,
            value,
        });

        value
    }

    /// Register a text input and return its current value.
    fn register_text_input(&mut self, id: &str, label: &str, placeholder: &str, default: &str) -> String {
        let value = self
            .values
            .get(id)
            .and_then(|v| v.as_string())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string());

        self.widgets.push(WidgetDef::TextInput {
            id: id.to_string(),
            label: label.to_string(),
            placeholder: placeholder.to_string(),
            value: value.clone(),
        });

        value
    }

    /// Register a select widget and return the currently selected option.
    fn register_select(&mut self, id: &str, label: &str, options: &[&str], default: usize) -> String {
        let selected = self
            .values
            .get(id)
            .and_then(|v| v.as_index())
            .unwrap_or(default)
            .min(options.len().saturating_sub(1));

        self.widgets.push(WidgetDef::Select {
            id: id.to_string(),
            label: label.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
            selected,
        });

        options.get(selected).map(|s| s.to_string()).unwrap_or_default()
    }

    /// Register a checkbox and return its current value.
    fn register_checkbox(&mut self, id: &str, label: &str, default: bool) -> bool {
        let value = self
            .values
            .get(id)
            .and_then(|v| v.as_bool())
            .unwrap_or(default);

        self.widgets.push(WidgetDef::Checkbox {
            id: id.to_string(),
            label: label.to_string(),
            value,
        });

        value
    }

    /// Get all registered widgets.
    pub fn take_widgets(&mut self) -> Vec<WidgetDef> {
        std::mem::take(&mut self.widgets)
    }
}

thread_local! {
    /// Thread-local widget context for the current cell execution.
    static WIDGET_CONTEXT: RefCell<Option<WidgetContext>> = const { RefCell::new(None) };
}

/// Set the widget context for the current cell execution.
///
/// This is called by the executor before calling the cell function.
pub fn set_widget_context(ctx: WidgetContext) {
    WIDGET_CONTEXT.with(|c| {
        *c.borrow_mut() = Some(ctx);
    });
}

/// Take the widget context after cell execution.
///
/// This is called by the executor after the cell function returns.
pub fn take_widget_context() -> Option<WidgetContext> {
    WIDGET_CONTEXT.with(|c| c.borrow_mut().take())
}

/// Access the widget context, returning a default if not set.
fn with_context<F, R>(f: F) -> R
where
    F: FnOnce(&mut WidgetContext) -> R,
{
    WIDGET_CONTEXT.with(|c| {
        let mut ctx = c.borrow_mut();
        if ctx.is_none() {
            *ctx = Some(WidgetContext::new());
        }
        f(ctx.as_mut().unwrap())
    })
}

// =============================================================================
// Public Widget API
// =============================================================================

/// Create a numeric slider widget.
///
/// Returns the current slider value (set by user or default).
/// When the user moves the slider, the cell automatically re-executes.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `min` - Minimum slider value
/// * `max` - Maximum slider value
/// * `default` - Default value when first rendered
///
/// # Example
///
/// ```rust,ignore
/// let speed = venus::input_slider("speed", 0.0, 100.0, 50.0);
/// println!("Current speed: {}", speed);
/// ```
pub fn input_slider(id: &str, min: f64, max: f64, default: f64) -> f64 {
    input_slider_with_step(id, min, max, 1.0, default)
}

/// Create a numeric slider widget with custom step.
///
/// Like `input_slider` but allows specifying the step increment.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `min` - Minimum slider value
/// * `max` - Maximum slider value
/// * `step` - Step increment for the slider
/// * `default` - Default value when first rendered
pub fn input_slider_with_step(id: &str, min: f64, max: f64, step: f64, default: f64) -> f64 {
    with_context(|ctx| ctx.register_slider(id, id, min, max, step, default))
}

/// Create a numeric slider widget with custom label.
///
/// Like `input_slider` but allows specifying a human-readable label.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `label` - Human-readable label shown in the UI
/// * `min` - Minimum slider value
/// * `max` - Maximum slider value
/// * `step` - Step increment for the slider
/// * `default` - Default value when first rendered
pub fn input_slider_labeled(id: &str, label: &str, min: f64, max: f64, step: f64, default: f64) -> f64 {
    with_context(|ctx| ctx.register_slider(id, label, min, max, step, default))
}

/// Create a text input widget.
///
/// Returns the current text value (set by user or default).
/// When the user changes the text, the cell automatically re-executes.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `placeholder` - Placeholder text shown when empty
///
/// # Example
///
/// ```rust,ignore
/// let name = venus::input_text("name", "Enter your name");
/// println!("Hello, {}!", name);
/// ```
pub fn input_text(id: &str, placeholder: &str) -> String {
    input_text_with_default(id, placeholder, "")
}

/// Create a text input widget with a default value.
///
/// Like `input_text` but allows specifying an initial value.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `placeholder` - Placeholder text shown when empty
/// * `default` - Default value when first rendered
pub fn input_text_with_default(id: &str, placeholder: &str, default: &str) -> String {
    with_context(|ctx| ctx.register_text_input(id, id, placeholder, default))
}

/// Create a text input widget with custom label.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `label` - Human-readable label shown in the UI
/// * `placeholder` - Placeholder text shown when empty
/// * `default` - Default value when first rendered
pub fn input_text_labeled(id: &str, label: &str, placeholder: &str, default: &str) -> String {
    with_context(|ctx| ctx.register_text_input(id, label, placeholder, default))
}

/// Create a dropdown select widget.
///
/// Returns the currently selected option as a string.
/// When the user selects a different option, the cell automatically re-executes.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `options` - List of available options
/// * `default` - Index of the default selection (0-based)
///
/// # Example
///
/// ```rust,ignore
/// let mode = venus::input_select("mode", &["Fast", "Normal", "Slow"], 1);
/// println!("Selected mode: {}", mode);
/// ```
pub fn input_select(id: &str, options: &[&str], default: usize) -> String {
    with_context(|ctx| ctx.register_select(id, id, options, default))
}

/// Create a dropdown select widget with custom label.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `label` - Human-readable label shown in the UI
/// * `options` - List of available options
/// * `default` - Index of the default selection (0-based)
pub fn input_select_labeled(id: &str, label: &str, options: &[&str], default: usize) -> String {
    with_context(|ctx| ctx.register_select(id, label, options, default))
}

/// Create a checkbox widget.
///
/// Returns the current boolean value.
/// When the user toggles the checkbox, the cell automatically re-executes.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `default` - Default value when first rendered
///
/// # Example
///
/// ```rust,ignore
/// let enabled = venus::input_checkbox("enabled", true);
/// if enabled {
///     println!("Feature is enabled!");
/// }
/// ```
pub fn input_checkbox(id: &str, default: bool) -> bool {
    with_context(|ctx| ctx.register_checkbox(id, id, default))
}

/// Create a checkbox widget with custom label.
///
/// # Arguments
///
/// * `id` - Unique identifier for this widget within the cell
/// * `label` - Human-readable label shown in the UI
/// * `default` - Default value when first rendered
pub fn input_checkbox_labeled(id: &str, label: &str, default: bool) -> bool {
    with_context(|ctx| ctx.register_checkbox(id, label, default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slider_registration() {
        let ctx = WidgetContext::new();
        set_widget_context(ctx);

        let value = input_slider("speed", 0.0, 100.0, 50.0);
        assert_eq!(value, 50.0);

        let ctx = take_widget_context().unwrap();
        assert_eq!(ctx.widgets.len(), 1);
        match &ctx.widgets[0] {
            WidgetDef::Slider { id, min, max, value, .. } => {
                assert_eq!(id, "speed");
                assert_eq!(*min, 0.0);
                assert_eq!(*max, 100.0);
                assert_eq!(*value, 50.0);
            }
            _ => panic!("Expected slider"),
        }
    }

    #[test]
    fn test_slider_with_existing_value() {
        let mut values = HashMap::new();
        values.insert("speed".to_string(), WidgetValue::Number(75.0));
        let ctx = WidgetContext::with_values(values);
        set_widget_context(ctx);

        let value = input_slider("speed", 0.0, 100.0, 50.0);
        assert_eq!(value, 75.0);

        let ctx = take_widget_context().unwrap();
        match &ctx.widgets[0] {
            WidgetDef::Slider { value, .. } => {
                assert_eq!(*value, 75.0);
            }
            _ => panic!("Expected slider"),
        }
    }

    #[test]
    fn test_text_input_registration() {
        let ctx = WidgetContext::new();
        set_widget_context(ctx);

        let value = input_text("name", "Enter name");
        assert_eq!(value, "");

        let ctx = take_widget_context().unwrap();
        assert_eq!(ctx.widgets.len(), 1);
        match &ctx.widgets[0] {
            WidgetDef::TextInput { id, placeholder, .. } => {
                assert_eq!(id, "name");
                assert_eq!(placeholder, "Enter name");
            }
            _ => panic!("Expected text input"),
        }
    }

    #[test]
    fn test_select_registration() {
        let ctx = WidgetContext::new();
        set_widget_context(ctx);

        let value = input_select("mode", &["Fast", "Normal", "Slow"], 1);
        assert_eq!(value, "Normal");

        let ctx = take_widget_context().unwrap();
        assert_eq!(ctx.widgets.len(), 1);
        match &ctx.widgets[0] {
            WidgetDef::Select { id, options, selected, .. } => {
                assert_eq!(id, "mode");
                assert_eq!(options, &["Fast", "Normal", "Slow"]);
                assert_eq!(*selected, 1);
            }
            _ => panic!("Expected select"),
        }
    }

    #[test]
    fn test_checkbox_registration() {
        let ctx = WidgetContext::new();
        set_widget_context(ctx);

        let value = input_checkbox("enabled", true);
        assert!(value);

        let ctx = take_widget_context().unwrap();
        assert_eq!(ctx.widgets.len(), 1);
        match &ctx.widgets[0] {
            WidgetDef::Checkbox { id, value, .. } => {
                assert_eq!(id, "enabled");
                assert!(*value);
            }
            _ => panic!("Expected checkbox"),
        }
    }

    #[test]
    fn test_widget_value_clamping() {
        let mut values = HashMap::new();
        values.insert("speed".to_string(), WidgetValue::Number(150.0)); // Over max
        let ctx = WidgetContext::with_values(values);
        set_widget_context(ctx);

        let value = input_slider("speed", 0.0, 100.0, 50.0);
        assert_eq!(value, 100.0); // Clamped to max

        take_widget_context();
    }
}
