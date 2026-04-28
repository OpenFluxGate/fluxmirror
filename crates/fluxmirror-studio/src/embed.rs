//! Compile-time embed of the Vite build artefact.
//!
//! Everything under `studio-web/dist/` is baked into the binary so the
//! single executable serves the SPA without any runtime file
//! dependency. The build.rs sibling makes sure the bundle is fresh
//! before the macro expands.

use include_dir::{include_dir, Dir};

pub static DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../studio-web/dist");

pub fn index_html() -> &'static [u8] {
    DIST.get_file("index.html")
        .expect("studio-web/dist/index.html missing — build.rs failed?")
        .contents()
}

pub fn lookup(path: &str) -> Option<(&'static [u8], &'static str)> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let file = DIST.get_file(trimmed)?;
    Some((file.contents(), mime_for(trimmed)))
}

fn mime_for(path: &str) -> &'static str {
    let lower = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match lower.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
