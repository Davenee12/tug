//! The HTTP side of Drop: serves the phone page and the small API it talks to. Plain HTTP on one
//! LAN address while Drop is open; every `/api` request is authenticated (`auth.rs`) and every
//! body is sealed (`crypto.rs`), so the network only ever sees ciphertext and chunk numbers.

use std::future::Future;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use tokio::net::TcpListener;

use super::auth::Authorized;
use super::crypto::OVERHEAD;
use super::page;
use super::session::{ApiError, Session, TextRequest, UploadRequest};
use super::upload::MAX_CHUNK;

type Shared = State<Arc<Session>>;

/// The page's own rules: nothing but its own scripts, styles and API; blob: for downloads.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; base-uri 'none'; \
                   form-action 'none'; frame-ancestors 'none'";

pub fn router(session: Arc<Session>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/assets/{file}", get(asset))
        .route("/api/state", get(state))
        .route("/api/text", post(text))
        .route("/api/up/{id}", post(begin_upload).delete(cancel_upload))
        .route("/api/up/{id}/{index}", put(put_chunk))
        .route("/api/finish/{id}", post(finish_upload))
        .route("/api/down/{id}/{index}", get(get_chunk))
        .fallback(|| async { StatusCode::NOT_FOUND })
        // A sealed chunk is the largest body there is.
        .layer(DefaultBodyLimit::max(MAX_CHUNK as usize + OVERHEAD + 4096))
        .with_state(session)
}

/// Serve until `shutdown` resolves.
pub async fn serve(listener: TcpListener, session: Arc<Session>, shutdown: impl Future<Output = ()> + Send + 'static) {
    if let Err(e) = axum::serve(listener, router(session))
        .with_graceful_shutdown(shutdown)
        .await
    {
        log::warn!("drop: server stopped: {e}");
    }
}

fn error(e: ApiError) -> Response {
    let status = StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        format!("{{\"error\":\"{}\"}}", e.code()),
    )
        .into_response()
}

fn sealed(bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response()
}

fn authorize(s: &Session, method: &Method, uri: &Uri, headers: &HeaderMap) -> Result<Authorized, ApiError> {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok());
    s.authorize(auth, method.as_str(), uri.path(), ua)
}

/// Run file work and crypto off the async threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.unwrap_or(Err(ApiError::Io))
}

async fn index(State(s): Shared) -> Response {
    if s.is_closed() {
        return error(ApiError::Closed);
    }
    s.touch();
    let Some((html, kind)) = page::asset("/index.html") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut res = ([(header::CONTENT_TYPE, kind)], html).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

async fn asset(State(s): Shared, Path(file): Path<String>) -> Response {
    if s.is_closed() {
        return error(ApiError::Closed);
    }
    match page::asset(&format!("/assets/{file}")) {
        Some((bytes, kind)) => (
            [
                (header::CONTENT_TYPE, kind),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "max-age=3600"),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn state(State(s): Shared, method: Method, uri: Uri, headers: HeaderMap) -> Response {
    match authorize(&s, &method, &uri, &headers) {
        Ok(req) => sealed(s.seal_json(&req, &s.page_state())),
        Err(e) => error(e),
    }
}

async fn text(State(s): Shared, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let run = || {
        let req = authorize(&s, &method, &uri, &headers)?;
        let msg: TextRequest = s.open_json(&req, &body)?;
        s.phone_text(msg.text)?;
        Ok(sealed(s.seal_json(&req, &serde_json::json!({ "ok": true }))))
    };
    run().unwrap_or_else(error)
}

async fn begin_upload(
    State(s): Shared,
    Path(id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let run = async {
        let req = authorize(&s, &method, &uri, &headers)?;
        let msg: UploadRequest = s.open_json(&req, &body)?;
        let s2 = s.clone();
        let reply = blocking(move || s2.begin_upload(&id, &msg)).await?;
        Ok(sealed(s.seal_json(&req, &reply)))
    };
    run.await.unwrap_or_else(error)
}

async fn put_chunk(
    State(s): Shared,
    Path((id, index)): Path<(String, u32)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let run = async {
        authorize(&s, &method, &uri, &headers)?;
        let s2 = s.clone();
        blocking(move || s2.write_chunk(&id, index, &body)).await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    };
    run.await.unwrap_or_else(error)
}

async fn cancel_upload(
    State(s): Shared,
    Path(id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let run = async {
        authorize(&s, &method, &uri, &headers)?;
        let s2 = s.clone();
        blocking(move || s2.cancel_upload(&id)).await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    };
    run.await.unwrap_or_else(error)
}

async fn finish_upload(
    State(s): Shared,
    Path(id): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let run = async {
        let req = authorize(&s, &method, &uri, &headers)?;
        let s2 = s.clone();
        let reply = blocking(move || s2.finish_upload(&id)).await?;
        Ok(sealed(s.seal_json(&req, &reply)))
    };
    run.await.unwrap_or_else(error)
}

async fn get_chunk(
    State(s): Shared,
    Path((id, index)): Path<(String, u32)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let run = async {
        authorize(&s, &method, &uri, &headers)?;
        let s2 = s.clone();
        let bytes = blocking(move || s2.read_offer_chunk(&id, index)).await?;
        Ok(sealed(bytes))
    };
    run.await.unwrap_or_else(error)
}
