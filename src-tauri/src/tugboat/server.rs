//! The HTTP side of Tugboat: serves the phone page and the small API it talks to. Plain HTTP on one
//! LAN address while Tugboat is open; every `/api` request is authenticated (`auth.rs`) and every
//! body is sealed (`crypto.rs`), so the network only ever sees ciphertext and chunk numbers.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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

/// The largest body there is: a sealed chunk.
const BODY_LIMIT: usize = MAX_CHUNK as usize + OVERHEAD + 4096;

fn router(session: Arc<Session>, body_timeout: Duration) -> Router {
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
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::Extension(BodyTimeout(body_timeout)))
        .with_state(session)
}

/// How long a request body may take (see `Limits::body_timeout`).
#[derive(Clone, Copy)]
struct BodyTimeout(Duration);

/// How much a connection may cost before the server has any idea who it is. Without these, any
/// device on the network could hold thousands of half-sent requests open (slowloris) and pin
/// memory in tug, no secret needed.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Time to send a request's headers; slower connections are closed.
    pub header_timeout: Duration,
    /// Time to send a body once its headers were accepted (a 2 MB chunk on poor Wi-Fi is slow, so
    /// this is generous).
    pub body_timeout: Duration,
    /// Connections open at once; more are closed as they arrive.
    pub max_connections: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            header_timeout: Duration::from_secs(10),
            body_timeout: Duration::from_secs(120),
            max_connections: 32,
        }
    }
}

/// Largest buffered request head a connection may hold.
const MAX_BUF: usize = 64 * 1024;

/// Serve until `shutdown` resolves.
pub async fn serve(listener: TcpListener, session: Arc<Session>, shutdown: impl Future<Output = ()> + Send + 'static) {
    serve_with(listener, session, shutdown, Limits::default()).await
}

/// Serve with explicit limits. Every connection is a task in one JoinSet, so when this returns
/// (Tugboat closed, moved to a new address, or went idle) every connection ends with it, mid-request
/// or not: nothing outlives the session.
pub async fn serve_with(
    listener: TcpListener,
    session: Arc<Session>,
    shutdown: impl Future<Output = ()> + Send + 'static,
    limits: Limits,
) {
    let app = router(session, limits.body_timeout);
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            _ = &mut shutdown => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
            accepted = listener.accept() => accepted,
        };
        // Accept errors are per-connection on Windows (a reset before accept, WSAECONNRESET):
        // never let one stop the server.
        let Ok((stream, _)) = accepted else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let Ok(slot) = slots.clone().try_acquire_owned() else {
            log::debug!("tugboat: too many connections, closing one");
            drop(stream);
            continue;
        };
        let service = TowerToHyperService::new(app.clone());
        connections.spawn(async move {
            let _slot = slot;
            let conn = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(limits.header_timeout)
                .max_buf_size(MAX_BUF)
                .serve_connection(TokioIo::new(stream), service);
            if let Err(e) = conn.await {
                log::debug!("tugboat: connection ended: {e}");
            }
        });
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
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

/// Authenticate a request from its headers, *then* read its body. A slow 2 MB chunk is checked
/// as of when it started, so it can't fall behind the replay window while it uploads (and a
/// stranger's body is never read at all).
async fn authorize_then_read(s: &Session, req: Request) -> Result<(Authorized, Bytes), ApiError> {
    let ok = authorize(s, req.method(), req.uri(), req.headers())?;
    let limit = req
        .extensions()
        .get::<BodyTimeout>()
        .map_or(Limits::default().body_timeout, |t| t.0);
    let body: Body = req.into_body();
    // A body that stalls (the phone left the Wi-Fi mid-chunk) gives up instead of waiting forever.
    let bytes = tokio::time::timeout(limit, to_bytes(body, BODY_LIMIT))
        .await
        .map_err(|_| ApiError::BadRequest)?
        .map_err(|_| ApiError::BadRequest)?;
    Ok((ok, bytes))
}

/// Run file work and crypto off the async threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.unwrap_or(Err(ApiError::Io))
}

async fn index(State(s): Shared) -> Response {
    if s.is_closed() {
        return error(ApiError::Closed);
    }
    // Not counted as activity: any device on the network could fetch the page and keep Tugboat on.
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

async fn text(State(s): Shared, req: Request) -> Response {
    let run = async {
        let (ok, body) = authorize_then_read(&s, req).await?;
        let msg: TextRequest = s.open_json(&ok, &body)?;
        s.phone_text(msg.text)?;
        Ok(sealed(s.seal_json(&ok, &serde_json::json!({ "ok": true }))))
    };
    run.await.unwrap_or_else(error)
}

async fn begin_upload(State(s): Shared, Path(id): Path<String>, req: Request) -> Response {
    let run = async {
        let (ok, body) = authorize_then_read(&s, req).await?;
        let msg: UploadRequest = s.open_json(&ok, &body)?;
        let s2 = s.clone();
        let reply = blocking(move || s2.begin_upload(&id, &msg)).await?;
        Ok(sealed(s.seal_json(&ok, &reply)))
    };
    run.await.unwrap_or_else(error)
}

async fn put_chunk(State(s): Shared, Path((id, index)): Path<(String, u32)>, req: Request) -> Response {
    let run = async {
        let (_, body) = authorize_then_read(&s, req).await?;
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
