//! Rich output rendering for notebook cells.
//!
//! The [`Render`] trait allows types to provide rich output representations
//! for display in the notebook UI.

use serde::Serialize;

/// Output from rendering a value.
#[derive(Debug, Clone)]
pub struct ImageOutput {
    /// MIME type (e.g., "image/png", "image/svg+xml")
    pub mime_type: String,
    /// Raw image data
    pub data: Vec<u8>,
}

/// Types that can render rich output in the notebook.
///
/// Implement this trait to customize how your types are displayed
/// in the Venus notebook UI.
///
/// # Default Implementation
///
/// By default, types use their [`Debug`] representation for text output.
/// Override specific methods to provide richer representations.
///
/// # Example
///
/// ```rust
/// use venus::render::{Render, ImageOutput};
///
/// struct MyChart {
///     data: Vec<f64>,
/// }
///
/// impl Render for MyChart {
///     fn render_text(&self) -> String {
///         format!("Chart with {} data points", self.data.len())
///     }
///
///     fn render_html(&self) -> Option<String> {
///         Some(format!(
///             "<div class='chart'>Chart: {:?}</div>",
///             self.data
///         ))
///     }
/// }
/// ```
pub trait Render {
    /// Plain text representation (for terminals).
    ///
    /// Defaults to the [`Debug`] representation if the type implements it.
    fn render_text(&self) -> String
    where
        Self: std::fmt::Debug,
    {
        format!("{:?}", self)
    }

    /// Rich HTML representation (for notebook frontend).
    ///
    /// Return `None` to fall back to text rendering.
    fn render_html(&self) -> Option<String> {
        None
    }

    /// Image output (PNG, SVG, etc.).
    ///
    /// Return `None` if this type doesn't render as an image.
    fn render_image(&self) -> Option<ImageOutput> {
        None
    }

    /// Structured data for programmatic access.
    ///
    /// Return `None` if no structured data is available.
    fn render_data(&self) -> Option<serde_json::Value> {
        None
    }
}

// Blanket implementations for common types

impl Render for String {
    fn render_text(&self) -> String {
        self.clone()
    }
}

impl Render for &str {
    fn render_text(&self) -> String {
        (*self).to_string()
    }
}

impl Render for i32 {
    fn render_text(&self) -> String {
        self.to_string()
    }
}

impl Render for i64 {
    fn render_text(&self) -> String {
        self.to_string()
    }
}

impl Render for f32 {
    fn render_text(&self) -> String {
        self.to_string()
    }
}

impl Render for f64 {
    fn render_text(&self) -> String {
        self.to_string()
    }
}

impl Render for bool {
    fn render_text(&self) -> String {
        self.to_string()
    }
}

impl<T: Render> Render for Vec<T>
where
    T: std::fmt::Debug,
{
    fn render_text(&self) -> String {
        format!("{:?}", self)
    }

    fn render_html(&self) -> Option<String> {
        let items: Vec<String> = self.iter().map(|item| item.render_text()).collect();
        Some(format!(
            "<ul>{}</ul>",
            items
                .iter()
                .map(|s| format!("<li>{}</li>", s))
                .collect::<String>()
        ))
    }
}

impl<T: Render> Render for Option<T>
where
    T: std::fmt::Debug,
{
    fn render_text(&self) -> String {
        match self {
            Some(v) => v.render_text(),
            None => "None".to_string(),
        }
    }
}

impl<T: Render, E: std::fmt::Debug> Render for Result<T, E>
where
    T: std::fmt::Debug,
{
    fn render_text(&self) -> String {
        match self {
            Ok(v) => v.render_text(),
            Err(e) => format!("Error: {:?}", e),
        }
    }
}

impl Render for serde_json::Value {
    fn render_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("{:?}", self))
    }

    fn render_html(&self) -> Option<String> {
        Some(format!(
            "<pre><code class=\"language-json\">{}</code></pre>",
            serde_json::to_string_pretty(self).unwrap_or_else(|_| format!("{:?}", self))
        ))
    }

    fn render_data(&self) -> Option<serde_json::Value> {
        Some(self.clone())
    }
}

/// Convenience wrapper for types that implement Serialize.
///
/// Wrapping a value in `Json` will render it as formatted JSON.
#[derive(Debug, Clone)]
pub struct Json<T: Serialize>(pub T);

impl<T: Serialize + std::fmt::Debug> Render for Json<T> {
    fn render_text(&self) -> String {
        serde_json::to_string_pretty(&self.0).unwrap_or_else(|_| format!("{:?}", self.0))
    }

    fn render_html(&self) -> Option<String> {
        Some(format!(
            "<pre><code class=\"language-json\">{}</code></pre>",
            serde_json::to_string_pretty(&self.0).unwrap_or_else(|_| format!("{:?}", self.0))
        ))
    }

    fn render_data(&self) -> Option<serde_json::Value> {
        serde_json::to_value(&self.0).ok()
    }
}

// =============================================================================
// Optional integrations (feature-gated)
// =============================================================================

/// Render implementation for polars DataFrame.
#[cfg(feature = "polars")]
mod polars_impl {
    use super::{ImageOutput, Render};

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    impl Render for polars::frame::DataFrame {
        fn render_text(&self) -> String {
            format!("{}", self)
        }

        fn render_html(&self) -> Option<String> {
            let (nrows, ncols) = self.shape();

            // Build HTML table
            let mut html = String::new();
            html.push_str("<table class=\"venus-dataframe\">\n");

            // Header row
            html.push_str("<thead><tr>");
            for name in self.get_column_names() {
                html.push_str(&format!("<th>{}</th>", html_escape(name)));
            }
            html.push_str("</tr></thead>\n");

            // Data rows (limit to first 100 rows for display)
            html.push_str("<tbody>\n");
            let display_rows = nrows.min(100);
            let columns = self.get_columns();

            for row_idx in 0..display_rows {
                html.push_str("<tr>");
                for col in columns.iter() {
                    // Use get which returns Result<AnyValue>, handle gracefully
                    let value = col
                        .get(row_idx)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|_| "[error]".to_string());
                    html.push_str(&format!("<td>{}</td>", html_escape(&value)));
                }
                html.push_str("</tr>\n");
            }
            html.push_str("</tbody>\n");

            // Footer with row count if truncated
            if nrows > display_rows {
                html.push_str(&format!(
                    "<tfoot><tr><td colspan=\"{}\">... {} more rows</td></tr></tfoot>\n",
                    ncols,
                    nrows - display_rows
                ));
            }

            html.push_str("</table>");
            Some(html)
        }

        fn render_image(&self) -> Option<ImageOutput> {
            None
        }

        fn render_data(&self) -> Option<serde_json::Value> {
            // Convert to JSON-compatible structure
            let mut cols = serde_json::Map::new();
            for col in self.get_columns() {
                let values: Vec<serde_json::Value> = (0..col.len())
                    .map(|i| {
                        col.get(i)
                            .map(|v| serde_json::Value::String(v.to_string()))
                            .unwrap_or(serde_json::Value::Null)
                    })
                    .collect();
                cols.insert(col.name().to_string(), serde_json::Value::Array(values));
            }
            Some(serde_json::Value::Object(cols))
        }
    }
}

/// Render implementation for image::DynamicImage.
#[cfg(feature = "image")]
mod image_impl {
    use super::{ImageOutput, Render};
    use std::io::Cursor;

    impl Render for image::DynamicImage {
        fn render_text(&self) -> String {
            let (width, height) = (self.width(), self.height());
            format!("Image({}x{}, {:?})", width, height, self.color())
        }

        fn render_html(&self) -> Option<String> {
            // Encode as base64 PNG for inline display
            let mut buf = Vec::new();
            let mut cursor = Cursor::new(&mut buf);
            self.write_to(&mut cursor, image::ImageFormat::Png).ok()?;

            let base64 = base64_encode(&buf);
            Some(format!(
                "<img src=\"data:image/png;base64,{}\" alt=\"Image {}x{}\" />",
                base64,
                self.width(),
                self.height()
            ))
        }

        fn render_image(&self) -> Option<ImageOutput> {
            let mut buf = Vec::new();
            let mut cursor = Cursor::new(&mut buf);
            self.write_to(&mut cursor, image::ImageFormat::Png).ok()?;

            Some(ImageOutput {
                mime_type: "image/png".to_string(),
                data: buf,
            })
        }

        fn render_data(&self) -> Option<serde_json::Value> {
            Some(serde_json::json!({
                "width": self.width(),
                "height": self.height(),
                "color_type": format!("{:?}", self.color())
            }))
        }
    }

    fn base64_encode(data: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

        for chunk in data.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
            let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

            result.push(ALPHABET[b0 >> 2] as char);
            result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

            if chunk.len() > 1 {
                result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
            } else {
                result.push('=');
            }

            if chunk.len() > 2 {
                result.push(ALPHABET[b2 & 0x3f] as char);
            } else {
                result.push('=');
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_render() {
        let s = String::from("hello");
        assert_eq!(s.render_text(), "hello");
    }

    #[test]
    fn test_vec_render() {
        let v = vec![1, 2, 3];
        assert_eq!(v.render_text(), "[1, 2, 3]");
        assert!(v.render_html().unwrap().contains("<ul>"));
    }

    #[test]
    fn test_json_render() {
        let j = Json(serde_json::json!({"key": "value"}));
        assert!(j.render_text().contains("key"));
        assert!(j.render_html().unwrap().contains("<pre>"));
    }
}
