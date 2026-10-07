//! A one-shot loopback HTTP listener for the OAuth redirect. It binds a loopback port, accepts
//! exactly the `/callback` request Spotify redirects to, shows a small "you can close this tab"
//! page, and returns the query string. Everything but the socket work is pure and tested.
//!
//! Spotify requires a loopback IP literal (`127.0.0.1`), never `localhost`, and permits HTTP
//! there. Its docs also allow registering it without a port, but the dashboard rejects that as
//! "not secure" (seen on a test account, 2026-10-05), so tug uses one fixed, registered port.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Which sign-in is the live one. Pressing Connect again while an earlier sign-in still waits
/// (up to 3 minutes) used to fail with "another app may be using port 8972" — tug itself was.
/// Now the new attempt takes over: the earlier wait sees it's no longer current and lets go of
/// the port.
static SIGN_IN: AtomicU64 = AtomicU64::new(0);

/// What an abandoned earlier sign-in returns once a newer one takes over.
pub const SUPERSEDED: &str = "Started a new Spotify sign-in.";

/// Start a sign-in, superseding any earlier one still waiting. Returns its ticket.
pub fn begin_sign_in() -> u64 {
    SIGN_IN.fetch_add(1, Ordering::SeqCst) + 1
}

/// Whether `ticket` is still the live sign-in.
pub fn is_current(ticket: u64) -> bool {
    SIGN_IN.load(Ordering::SeqCst) == ticket
}

/// The redirect path tug listens on.
pub const REDIRECT_PATH: &str = "/callback";
/// The one port tug listens on; it's part of the registered Redirect URI, so it can't vary.
pub const REDIRECT_PORT: u16 = 8972;
/// The redirect URI to register in the Spotify dashboard, exactly.
pub const REGISTERED_REDIRECT: &str = "http://127.0.0.1:8972/callback";

/// How long to wait for the user to finish signing in before giving up.
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

/// The request target (path + query) from an HTTP request line, for a `GET`.
/// `"GET /callback?code=x HTTP/1.1"` → `"/callback?code=x"`. `None` for anything else.
pub fn parse_request_target(request_line: &str) -> Option<&str> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method.eq_ignore_ascii_case("GET")).then_some(target)
}

/// The query part of a target (`/callback?a=b` → `a=b`), empty when there's none.
pub fn query_of(target: &str) -> &str {
    target.split_once('?').map(|(_, q)| q).unwrap_or("")
}

/// Whether a target is the OAuth callback path (ignoring the query).
pub fn is_callback(target: &str) -> bool {
    target.split(['?', '#']).next() == Some(REDIRECT_PATH)
}

/// The page shown in the browser once the code is captured.
fn done_page(ok: bool) -> String {
    let (heading, note) = if ok {
        (
            "tug is connected to Spotify",
            "You can close this tab and go back to tug.",
        )
    } else {
        (
            "Spotify sign-in didn't complete",
            "You can close this tab and try again in tug.",
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>tug</title>\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>body{{font-family:-apple-system,Segoe UI,system-ui,sans-serif;background:#faf6ef;\
         color:#2b2722;display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}}\
         .card{{text-align:center;max-width:22rem;padding:2rem}}h1{{font-size:1.25rem;margin:0 0 .5rem}}\
         p{{color:#6b6459;margin:0}}</style></head>\
         <body><div class=\"card\"><h1>{heading}</h1><p>{note}</p></div></body></html>"
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// A 404 for anything that isn't the callback (e.g. the browser's `/favicon.ico`), so the loop
/// keeps waiting for the real redirect instead of treating a stray request as the answer.
fn not_found() -> &'static str {
    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
}

/// Bind `REDIRECT_PORT` on `127.0.0.1` for the sign-in redirect. An earlier sign-in that was just
/// superseded releases the port within its next poll (100 ms), so a short retry covers it.
pub fn bind() -> Result<TcpListener, String> {
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match TcpListener::bind(("127.0.0.1", REDIRECT_PORT)) {
            Ok(l) => return Ok(l),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                return Err(format!(
                    "Couldn't listen for Spotify's sign-in on port {REDIRECT_PORT} (another app may be using it): {e}"
                ))
            }
        }
    }
}

/// Accept connections until the `/callback` arrives (or the deadline passes), and return its
/// query string. Non-callback requests are answered with 404 and ignored.
/// Gives up (releasing the port) as soon as a newer sign-in starts.
pub fn wait_for_callback(listener: TcpListener, timeout: Duration, ticket: u64) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("loopback listener error: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        if !is_current(ticket) {
            return Err(SUPERSEDED.into());
        }
        if Instant::now() >= deadline {
            return Err("Spotify sign-in timed out. Please try again.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(query) = handle(&mut stream) {
                    return Ok(query);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("loopback listener error: {e}")),
        }
    }
}

/// Read one request, and if it's the callback, answer with the done page and return its query.
/// Any other request gets a 404 and `None` (keep waiting).
fn handle(stream: &mut std::net::TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let line = text.lines().next().unwrap_or("");
    let target = match parse_request_target(line) {
        Some(t) if is_callback(t) => t,
        _ => {
            let _ = stream.write_all(not_found().as_bytes());
            return None;
        }
    };
    let query = query_of(target).to_string();
    let _ = stream.write_all(done_page(!query.is_empty()).as_bytes());
    let _ = stream.flush();
    Some(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_request_target_for_get_only() {
        assert_eq!(
            parse_request_target("GET /callback?code=x&state=y HTTP/1.1"),
            Some("/callback?code=x&state=y")
        );
        assert_eq!(parse_request_target("get /callback HTTP/1.0"), Some("/callback"));
        assert_eq!(parse_request_target("POST /callback HTTP/1.1"), None);
        assert_eq!(parse_request_target(""), None);
        assert_eq!(parse_request_target("GET"), None);
    }

    #[test]
    fn splits_query_and_recognises_the_callback() {
        assert_eq!(query_of("/callback?code=x&state=y"), "code=x&state=y");
        assert_eq!(query_of("/callback"), "");
        assert!(is_callback("/callback?code=x"));
        assert!(is_callback("/callback"));
        assert!(!is_callback("/favicon.ico"));
        assert!(!is_callback("/other?/callback"));
    }

    #[test]
    fn a_new_sign_in_supersedes_a_waiting_one() {
        let first = begin_sign_in();
        assert!(is_current(first));
        let second = begin_sign_in();
        assert!(!is_current(first) && is_current(second));
        // The earlier wait lets go at once instead of holding the port for 3 minutes.
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let started = Instant::now();
        assert_eq!(
            wait_for_callback(l, CALLBACK_TIMEOUT, first),
            Err(SUPERSEDED.to_string())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn registered_redirect_matches_the_listener() {
        assert_eq!(
            REGISTERED_REDIRECT,
            format!("http://127.0.0.1:{REDIRECT_PORT}{REDIRECT_PATH}")
        );
    }
}
