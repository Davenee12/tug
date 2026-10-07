//! tug's side of the bridge: a named pipe only the current user can open (never a network port),
//! the handshake, and handing checked calls to the app. What each call does lives in the app
//! (`src-tauri/src/devtools`); this module only enforces the protocol, auth and time limits.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::time::timeout;

use crate::auth;
use crate::framing::{read_msg, write_msg, FrameError};
use crate::protocol::{
    BridgeError, ClientInfo, ClientMsg, ErrorCode, Request, ServerMsg, MAX_CLIENT_LINE, PROTOCOL_VERSION,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How long a client gets to say hello, and then to send its call.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long writing the answer may take.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Most connections served at once; more wait for a free slot.
pub const MAX_CONNECTIONS: usize = 8;
/// What tug says when it's switched off.
pub const OFF_MESSAGE: &str =
    "AI tools are turned off in tug. Turn on Settings › Developer tools › Let AI tools use tug, then try again.";
pub const VERSION_MESSAGE: &str =
    "This tug command comes from a different version of tug. Update tug (or this command) and try again.";
pub const REVOKED_MESSAGE: &str =
    "tug's access for AI tools was reset with Revoke access. Restart this tool (or run the command again) to reconnect.";

/// The app behind the bridge.
pub trait Handler: Send + Sync + 'static {
    /// Settings › Developer tools › "Let AI tools use tug".
    fn enabled(&self) -> bool;
    /// The current token; `None` until Developer tools have been switched on once.
    fn token(&self) -> Option<String>;
    fn now_ms(&self) -> i64;
    /// Longest the app may take to answer this request (a text waits for the person to click).
    fn time_limit(&self, request: &Request) -> Duration;
    /// A call failed the token check (logged by the app, without anything the caller sent).
    fn rejected(&self, client: &ClientInfo, why: ErrorCode);
    fn handle(&self, client: ClientInfo, request: Request) -> BoxFuture<'_, Result<serde_json::Value, BridgeError>>;
}

async fn send<W: AsyncWrite + Unpin>(w: &mut W, msg: &ServerMsg) {
    let _ = timeout(WRITE_TIMEOUT, write_msg(w, msg)).await;
}

async fn fail<W: AsyncWrite + Unpin>(w: &mut W, code: ErrorCode, message: &str) {
    send(w, &ServerMsg::Error(BridgeError::new(code, message))).await;
}

/// Serve one connection: hello → challenge → call → answer. Any stream works, so the whole
/// exchange is tested over an in-memory pipe.
pub async fn handle_connection<S: AsyncRead + AsyncWrite + Send + Unpin>(stream: S, handler: &dyn Handler) {
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r);

    let hello = match timeout(HANDSHAKE_TIMEOUT, read_msg::<ClientMsg, _>(&mut r, MAX_CLIENT_LINE)).await {
        Ok(Ok(m)) => m,
        Ok(Err(FrameError::Closed)) | Err(_) => return,
        Ok(Err(_)) => return fail(&mut w, ErrorCode::Invalid, "That wasn't a tug bridge message.").await,
    };
    let ClientMsg::Hello {
        v,
        nonce: client_nonce,
        client,
    } = hello
    else {
        return fail(&mut w, ErrorCode::Invalid, "Say hello first.").await;
    };
    if v != PROTOCOL_VERSION {
        return fail(&mut w, ErrorCode::Version, VERSION_MESSAGE).await;
    }
    if !auth::valid_nonce(&client_nonce) {
        return fail(&mut w, ErrorCode::Invalid, "Bad hello.").await;
    }
    // Off answers before any auth: nothing about the user is revealed, and the caller can say why.
    let token = match (handler.enabled(), handler.token()) {
        (true, Some(t)) => t,
        _ => return fail(&mut w, ErrorCode::Off, OFF_MESSAGE).await,
    };
    let server_nonce = auth::new_nonce();
    let challenge = ServerMsg::Challenge {
        v: PROTOCOL_VERSION,
        proof: auth::server_proof(&token, &client_nonce, &server_nonce),
        nonce: server_nonce.clone(),
    };
    send(&mut w, &challenge).await;

    let msg = match timeout(HANDSHAKE_TIMEOUT, read_msg::<ClientMsg, _>(&mut r, MAX_CLIENT_LINE)).await {
        Ok(Ok(m)) => m,
        Ok(Err(FrameError::Closed)) | Err(_) => return,
        Ok(Err(FrameError::BadJson(_))) => {
            return fail(
                &mut w,
                ErrorCode::Invalid,
                "That's not a tool tug knows, or its arguments are wrong.",
            )
            .await
        }
        Ok(Err(_)) => return fail(&mut w, ErrorCode::Invalid, "That call is too big.").await,
    };
    let ClientMsg::Call { proof, call } = msg else {
        return fail(&mut w, ErrorCode::Invalid, "Expected a call.").await;
    };
    if !auth::verify_client(&token, &client_nonce, &server_nonce, &proof) {
        handler.rejected(&client, ErrorCode::Unauthorized);
        return fail(&mut w, ErrorCode::Unauthorized, REVOKED_MESSAGE).await;
    }
    let request = match call.validate(handler.now_ms()) {
        Ok(r) => r,
        Err(e) => return send(&mut w, &ServerMsg::Error(e)).await,
    };
    // Switched off while this connection was opening.
    if !handler.enabled() {
        return fail(&mut w, ErrorCode::Off, OFF_MESSAGE).await;
    }
    let limit = handler.time_limit(&request);
    // An approved text is saved and queued before anything waits on the phone: past the limit it
    // may still go, so the caller must not be told to try again.
    let is_send = matches!(request, Request::SendText { .. });
    let answer = match timeout(limit, handler.handle(client, request)).await {
        Ok(Ok(value)) => ServerMsg::Result { ok: value },
        Ok(Err(e)) => ServerMsg::Error(e),
        Err(_) if is_send => ServerMsg::Error(BridgeError::new(
            ErrorCode::Internal,
            crate::client::SEND_TIMEOUT_MESSAGE,
        )),
        Err(_) => ServerMsg::Error(BridgeError::new(
            ErrorCode::Internal,
            "tug took too long to answer. Try again.",
        )),
    };
    send(&mut w, &answer).await;
}

/// Listen on `pipe_name` until `shutdown` resolves. The pipe's ACL lets only `user_sid` in (and
/// names it the owner, which clients check), remote clients are refused, and creating the first
/// instance fails if anything else already holds the name: that's the only fatal error. Making a
/// later instance can fail for a moment (resources); that's logged and retried with a backoff
/// instead of ending the bridge.
#[cfg(windows)]
pub async fn serve(
    pipe_name: String,
    user_sid: String,
    handler: std::sync::Arc<dyn Handler>,
    shutdown: impl Future<Output = ()> + Send,
) -> std::io::Result<()> {
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::ServerOptions;
    use tokio::sync::Semaphore;

    let sec = crate::win::UserOnly::pipe(&user_sid)?;
    let mut opts = ServerOptions::new();
    opts.reject_remote_clients(true).first_pipe_instance(true);
    // SAFETY: `sec` outlives every call; the attributes are only read during creation.
    let mut server = unsafe { opts.create_with_security_attributes_raw(&pipe_name, sec.as_raw())? };
    opts.first_pipe_instance(false);
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    tokio::pin!(shutdown);
    loop {
        let permit = tokio::select! {
            p = slots.clone().acquire_owned() => p.expect("semaphore never closes"),
            _ = &mut shutdown => return Ok(()),
        };
        let connected = tokio::select! {
            r = server.connect() => r,
            _ = &mut shutdown => return Ok(()),
        };
        // A fresh instance for the next client. Until one exists, new clients see the pipe as
        // busy and retry for a few seconds, then give up with a plain message.
        let mut backoff = Duration::from_millis(250);
        let next = loop {
            // SAFETY: as above.
            match unsafe { opts.create_with_security_attributes_raw(&pipe_name, sec.as_raw()) } {
                Ok(s) => break s,
                Err(e) => {
                    log::warn!("devtools bridge: couldn't open another pipe instance ({e}); retrying");
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {}
                        _ = &mut shutdown => return Ok(()),
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        };
        let conn = std::mem::replace(&mut server, next);
        if connected.is_err() {
            // The client went away between connecting and being accepted.
            continue;
        }
        let handler = handler.clone();
        tokio::spawn(async move {
            handle_connection(conn, &*handler).await;
            drop(permit);
        });
    }
}
