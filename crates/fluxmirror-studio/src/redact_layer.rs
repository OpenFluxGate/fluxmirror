//! Output-surface scrubber for fluxmirror-studio.
//!
//! A single tower middleware that wraps every outbound response and, if
//! the `Content-Type` is text-shaped (HTML / JSON / plain text), runs
//! the body through `fluxmirror_core::redact::scrub` before it leaves
//! the process. CSS / JavaScript / image bytes are forwarded
//! unmodified so the SPA bundle still loads, even if a stylesheet
//! happens to literal-match a redaction pattern.
//!
//! The `events.db` file the studio reads is never touched — the studio
//! is read-only against SQLite, and scrubbing happens entirely on the
//! HTTP boundary.
//!
//! Phase 3 M7.

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::Response,
};
use fluxmirror_core::redact::{scrub, RedactionRules};

/// Maximum body size we'll buffer before giving up. Studio responses
/// are tiny dashboards (heatmaps, JSON payloads), so 16 MiB is well
/// past anything the embedded SPA emits — but the cap avoids a memory
/// blow-up on a hypothetical pathological route.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Tower middleware. Buffers the inner handler's body when its
/// Content-Type is one of the text-shaped types; otherwise passes the
/// response through with its body untouched.
pub async fn scrub_response(
    State(rules): State<Arc<RedactionRules>>,
    req: Request,
    next: Next,
) -> Response {
    let resp = next.run(req).await;
    if !is_textual(&resp) {
        return resp;
    }

    let (mut parts, body) = resp.into_parts();
    let bytes = match to_bytes(body, MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            // Body too large or stream error — surface an empty body
            // rather than risk leaking the unscrubbed bytes downstream.
            return Response::from_parts(parts, Body::empty());
        }
    };

    // If the body is non-UTF-8 (binary tagged with text/plain, say),
    // fall back to the raw bytes — scrubbing them blindly would
    // corrupt a real binary payload.
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };

    let scrubbed = scrub(text, &rules);
    let new_body = match scrubbed {
        std::borrow::Cow::Borrowed(_) => bytes.to_vec(),
        std::borrow::Cow::Owned(s) => s.into_bytes(),
    };

    // The new body length almost always differs from the old one;
    // remove the stale Content-Length header and let axum's
    // Body::from(Vec) compute the new size.
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(new_body))
}

/// Treat HTML / JSON / plain text as scrub-eligible. Everything else
/// (CSS, JS, images, fonts, Wasm) is forwarded byte-for-byte.
fn is_textual(resp: &Response) -> bool {
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    ct.starts_with("application/json")
        || ct.starts_with("text/html")
        || ct.starts_with("text/plain")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Response as HttpResponse, StatusCode};

    fn resp_with_ct(ct: &str) -> Response {
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ct)
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn is_textual_recognises_json_html_text() {
        assert!(is_textual(&resp_with_ct("application/json")));
        assert!(is_textual(&resp_with_ct("application/json; charset=utf-8")));
        assert!(is_textual(&resp_with_ct("text/html")));
        assert!(is_textual(&resp_with_ct("text/html; charset=utf-8")));
        assert!(is_textual(&resp_with_ct("text/plain")));
    }

    #[test]
    fn is_textual_rejects_css_js_image() {
        assert!(!is_textual(&resp_with_ct("text/css")));
        assert!(!is_textual(&resp_with_ct("application/javascript")));
        assert!(!is_textual(&resp_with_ct("text/javascript")));
        assert!(!is_textual(&resp_with_ct("image/png")));
        assert!(!is_textual(&resp_with_ct("font/woff2")));
        assert!(!is_textual(&resp_with_ct("application/wasm")));
    }

    #[test]
    fn is_textual_no_content_type_means_no_scrub() {
        let resp = HttpResponse::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
        assert!(!is_textual(&resp));
    }
}
