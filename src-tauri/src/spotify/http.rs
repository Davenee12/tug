//! HTTPS for the Spotify connector, over the same WinRT `HttpClient` tug already uses for App
//! Store icons (`app_icons.rs`) — no new HTTP stack, and nothing routed through the web view, so
//! no new CSP holes. This adds the verbs and headers Spotify needs (bearer auth, POST/PUT/DELETE
//! with a body) plus reading the status code and `Retry-After`.

/// The HTTP verbs tug sends to Spotify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

/// A response tug acts on: the status (for error classification and 401 refresh), the body, and
/// the parsed `Retry-After` seconds when the server sent one (429 handling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<u64>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub use imp::{get_bytes, random_bytes, request};

#[cfg(windows)]
mod imp {
    use std::time::Duration;

    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Storage::Streams::UnicodeEncoding;
    use windows::Web::Http::Headers::HttpCredentialsHeaderValue;
    use windows::Web::Http::{HttpClient, HttpMethod, HttpRequestMessage, HttpResponseMessage, HttpStringContent};

    use super::{Method, Response};

    const TIMEOUT: Duration = Duration::from_secs(20);

    fn err(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    /// Run a WinRT future on a private current-thread runtime, as `app_icons` does.
    fn run<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(f)
    }

    fn http_method(m: Method) -> windows::core::Result<HttpMethod> {
        match m {
            Method::Get => HttpMethod::Get(),
            Method::Post => HttpMethod::Post(),
            Method::Put => HttpMethod::Put(),
            Method::Delete => HttpMethod::Delete(),
        }
    }

    /// Send one request. `body` is `(media_type, text)`; `bearer` is the access token, if any.
    pub fn request(
        m: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<(&str, String)>,
    ) -> Result<Response, String> {
        run(async move {
            let client = HttpClient::new().map_err(err)?;
            let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(err)?;
            let req = HttpRequestMessage::new().map_err(err)?;
            req.SetRequestUri(&uri).map_err(err)?;
            req.SetMethod(&http_method(m).map_err(err)?).map_err(err)?;
            if let Some(token) = bearer {
                let auth = HttpCredentialsHeaderValue::CreateFromSchemeWithToken(
                    &HSTRING::from("Bearer"),
                    &HSTRING::from(token),
                )
                .map_err(err)?;
                req.Headers().map_err(err)?.SetAuthorization(&auth).map_err(err)?;
            }
            if let Some((media_type, data)) = body {
                let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(
                    &HSTRING::from(data),
                    UnicodeEncoding::Utf8,
                    &HSTRING::from(media_type),
                )
                .map_err(err)?;
                req.SetContent(&content).map_err(err)?;
            }
            let op = client.SendRequestAsync(&req).map_err(err)?;
            let resp = tokio::time::timeout(TIMEOUT, op)
                .await
                .map_err(|_| "Spotify request timed out".to_string())?
                .map_err(err)?;
            let status = resp.StatusCode().map_err(err)?.0 as u16;
            let retry_after = retry_after(&resp);
            let body_op = resp.Content().map_err(err)?.ReadAsStringAsync().map_err(err)?;
            let text = tokio::time::timeout(TIMEOUT, body_op)
                .await
                .map_err(|_| "Spotify response timed out".to_string())?
                .map_err(err)?;
            Ok(Response {
                status,
                body: text.to_string(),
                retry_after,
            })
        })
    }

    fn retry_after(resp: &HttpResponseMessage) -> Option<u64> {
        let key = HSTRING::from("Retry-After");
        let headers = resp.Headers().ok()?;
        if headers.HasKey(&key).unwrap_or(false) {
            headers.Lookup(&key).ok()?.to_string().trim().parse::<u64>().ok()
        } else {
            None
        }
    }

    /// Download bytes (album art) without auth, as `app_icons` fetches icons.
    pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
        run(async move {
            let client = HttpClient::new().map_err(err)?;
            let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(err)?;
            let op = client.GetBufferAsync(&uri).map_err(err)?;
            let buf = tokio::time::timeout(TIMEOUT, op)
                .await
                .map_err(|_| "album art download timed out".to_string())?
                .map_err(err)?;
            crate::ble::winrt::from_buffer(&buf).map_err(err)
        })
    }

    /// Cryptographically-random bytes (PKCE verifier, OAuth state) from Windows' CNG.
    pub fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
        let buf = windows::Security::Cryptography::CryptographicBuffer::GenerateRandom(n as u32).map_err(err)?;
        crate::ble::winrt::from_buffer(&buf).map_err(err)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Method, Response};

    pub fn request(
        _m: Method,
        _url: &str,
        _bearer: Option<&str>,
        _body: Option<(&str, String)>,
    ) -> Result<Response, String> {
        Err("Spotify is only supported on Windows".into())
    }
    pub fn get_bytes(_url: &str) -> Result<Vec<u8>, String> {
        Err("Spotify is only supported on Windows".into())
    }
    pub fn random_bytes(_n: usize) -> Result<Vec<u8>, String> {
        Err("Spotify is only supported on Windows".into())
    }
}
