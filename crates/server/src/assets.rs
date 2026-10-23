//! The built frontend (`frontend/dist`), embedded in release builds and read
//! from disk in debug builds. Unknown non-asset paths get `index.html` (SPA).

use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
#[allow_missing = true]
struct Assets;

const NOT_BUILT: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Plover</title></head>\
<body><p>UI not built &mdash; run <code>npm run build</code> in <code>frontend/</code>, then rebuild Plover.</p></body></html>";

/// Monaco needs blob workers and inline styles; the API and WebSocket are same-host.
const CSP: &str = "default-src 'self'; connect-src 'self' ws://127.0.0.1:* ws://localhost:*; \
style-src 'self' 'unsafe-inline'; worker-src 'self' blob:; img-src 'self' data:; font-src 'self' data:";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let mut response = match Assets::get(path).filter(|_| !path.is_empty()) {
        Some(file) => {
            let mut res = file.data.into_owned().into_response();
            if let Ok(mime) = HeaderValue::from_str(file.metadata.mimetype()) {
                res.headers_mut().insert(header::CONTENT_TYPE, mime);
            }
            res.headers_mut()
                .insert(header::CACHE_CONTROL, cache_control(path));
            res
        }
        // Hashed bundle files that are missing are real 404s, not app routes.
        None if path.starts_with("assets/") => StatusCode::NOT_FOUND.into_response(),
        None => index(),
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn index() -> Response {
    let mut res = match Assets::get("index.html") {
        Some(file) => Html(file.data.into_owned()).into_response(),
        None => Html(NOT_BUILT).into_response(),
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res
}

/// Vite puts content-hashed files under `assets/`; everything else may change.
fn cache_control(path: &str) -> HeaderValue {
    if path.starts_with("assets/") {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        HeaderValue::from_static("no-cache")
    }
}
