use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;

macro_rules! asset {
    ($path:literal, $file:literal, $type:expr) => {
        (
            $path,
            $type,
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", $file)).as_slice(),
        )
    };
}

const HTML: &str = "text/html; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";

pub const ASSETS: &[(&str, &str, &[u8])] = &[
    asset!("/", "web/index.html", HTML),
    asset!("/setup", "web/index.html", HTML),
    asset!("/signup", "web/index.html", HTML),
    asset!("/connect", "web/index.html", HTML),
    asset!("/add", "web/index.html", HTML),
    asset!("/admin", "web/index.html", HTML),
    asset!("/app.css", "web/app.css", "text/css; charset=utf-8"),
    asset!("/app.js", "web/app.js", JS),
    asset!("/board.js", "web/board.js", JS),
    asset!("/connect.js", "web/connect.js", JS),
    asset!("/pages.js", "web/pages.js", JS),
    asset!("/hub.js", "web/hub.js", JS),
    asset!("/keystore.js", "web/keystore.js", JS),
    asset!("/pairing.js", "web/pairing.js", JS),
    asset!("/passkey.js", "web/passkey.js", JS),
    asset!("/notice.js", "web/notice.js", JS),
    asset!("/sw.js", "web/sw.js", JS),
    asset!(
        "/manifest.webmanifest",
        "web/manifest.webmanifest",
        "application/manifest+json"
    ),
    asset!("/icon.svg", "web/icon.svg", "image/svg+xml"),
    asset!("/icon-192.png", "web/icon-192.png", "image/png"),
    asset!("/icon-512.png", "web/icon-512.png", "image/png"),
    asset!(
        "/apple-touch-icon.png",
        "web/apple-touch-icon.png",
        "image/png"
    ),
    asset!("/js/protocol.js", "js/protocol.js", JS),
    asset!("/js/hpke.js", "js/hpke.js", JS),
];

const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub fn find(path: &str) -> Option<(&'static str, &'static [u8])> {
    let path = if path == "/index.html" { "/" } else { path };
    ASSETS
        .iter()
        .find(|(served, _, _)| *served == path)
        .map(|(_, content_type, bytes)| (*content_type, *bytes))
}

pub async fn asset(method: Method, uri: Uri) -> Response {
    let found = (method == Method::GET || method == Method::HEAD)
        .then(|| find(uri.path()))
        .flatten();
    let Some((content_type, bytes)) = found else {
        return ApiError::not_found().into_response();
    };
    let headers = [
        (CONTENT_TYPE, HeaderValue::from_static(content_type)),
        (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ),
        (
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ),
        (
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ),
    ];
    let body = if method == Method::HEAD {
        &[][..]
    } else {
        bytes
    };
    (StatusCode::OK, headers, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_and_pages_carry_their_types() {
        assert_eq!(find("/index.html").map(|(kind, _)| kind), Some(HTML));
        assert_eq!(find("/js/protocol.js").map(|(kind, _)| kind), Some(JS));
        assert_eq!(find("/v1/d/boards"), None);
        for page in ["/setup", "/signup", "/connect", "/add", "/admin"] {
            assert_eq!(find(page), find("/"), "{page}");
        }
    }
}
