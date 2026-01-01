//! Embedded frontend assets for Venus server.
//!
//! This module provides embedded static files for the Venus notebook UI.
//! It is only available when the `embedded-frontend` feature is enabled.

use axum::{
    body::Body,
    http::{header, Response, StatusCode},
};
use rust_embed::Embed;

/// Embedded frontend assets.
#[derive(Embed)]
#[folder = "src/frontend/"]
pub struct FrontendAssets;

/// Serve an embedded frontend file.
pub fn serve_static(path: String) -> Response<Body> {
    // Remove leading slash if present
    let path = path.strip_prefix('/').map(|s| s.to_string()).unwrap_or(path);

    match FrontendAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, "public, max-age=3600")
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    }
}

/// Serve the main index.html file.
pub fn serve_index() -> Response<Body> {
    serve_static("index.html".to_string())
}

/// Check if the frontend assets are available.
pub fn is_available() -> bool {
    FrontendAssets::get("index.html").is_some()
}

/// List all embedded files (for debugging).
pub fn list_files() -> Vec<String> {
    FrontendAssets::iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_assets_available() {
        assert!(is_available(), "index.html should be embedded");
    }

    #[test]
    fn test_list_files() {
        let files = list_files();
        assert!(files.iter().any(|f| f == "index.html"));
        assert!(files.iter().any(|f| f == "app.js"));
        assert!(files.iter().any(|f| f == "styles.css"));
        assert!(files.iter().any(|f| f == "graph.js"));
        assert!(files.iter().any(|f| f == "lsp-client.js"));
    }
}
