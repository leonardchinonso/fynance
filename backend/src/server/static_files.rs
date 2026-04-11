//! Serve the compiled React frontend out of the binary.
//!
//! `include_dir!` reads `frontend/dist` at compile time and embeds it
//! in the Rust binary, so a release build of `fynance` is a single
//! self-contained file that opens a working UI when run. Any request
//! that does not map to a file in the bundle falls back to
//! `index.html` so React Router's client-side routes resolve.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use include_dir::{Dir, include_dir};

static FRONTEND_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist");

/// Axum fallback handler: resolve `uri.path()` against the embedded
/// bundle, falling back to `index.html` so SPA routes don't 404.
pub async fn serve_static(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    respond_from_bundle(path).unwrap_or_else(respond_index_html)
}

fn respond_from_bundle(path: &str) -> Option<Response> {
    let file = FRONTEND_DIR.get_file(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(file.contents()))
            .unwrap(),
    )
}

fn respond_index_html() -> Response {
    match FRONTEND_DIR.get_file("index.html") {
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(file.contents()))
            .unwrap(),
        None => (
            StatusCode::NOT_FOUND,
            "frontend bundle missing: build it with `cd frontend && npm run build`",
        )
            .into_response(),
    }
}
