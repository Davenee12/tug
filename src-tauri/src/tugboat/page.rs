//! The phone page, embedded in the binary. `npm run build` builds it (Vite, `vite.tugboat.config.ts`)
//! into `src-tauri/tugboat-page/dist`, and `build.rs` turns that folder into the `ASSETS` table below.
//! Without a build (a bare `cargo test`), the table holds a one-line placeholder page instead.

include!(concat!(env!("OUT_DIR"), "/tugboat_page.rs"));

/// The embedded file at `path` ("/index.html", "/assets/…") and its content type.
pub fn asset(path: &str) -> Option<(&'static [u8], &'static str)> {
    ASSETS
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(p, bytes)| (*bytes, content_type(p)))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_has_an_index_page() {
        let (html, kind) = asset("/index.html").unwrap();
        assert!(kind.starts_with("text/html"));
        assert!(std::str::from_utf8(html).unwrap().contains("tug"));
        assert!(asset("/../secret").is_none());
    }
}
