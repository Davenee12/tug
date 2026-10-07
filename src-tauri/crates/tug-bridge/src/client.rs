//! The companion side: the `tug` command and `tug mcp` open the pipe, check that tug really is
//! tug (its proof), prove they hold the token, make one call and read the answer. Every step has
//! a time limit, and every failure has a plain sentence for people.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::time::timeout;

use crate::auth;
use crate::framing::{read_msg, write_msg, FrameError};
use crate::protocol::{
    BridgeError, Call, ClientInfo, ClientMsg, ErrorCode, ServerMsg, MAX_SERVER_LINE, PROTOCOL_VERSION,
};
use crate::token_file::{self, TokenError};

/// Waiting for a free pipe instance while tug is busy.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// A normal call, start to finish.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// `send_text` waits for the person to click Send (2 minutes) and then for the phone.
pub const SEND_TEXT_TIMEOUT: Duration = Duration::from_secs(200);

pub const NOT_RUNNING: &str = "tug isn't running. Open tug and try again.";
pub const NOT_SET_UP: &str =
    "tug's Developer tools aren't set up yet. Open tug › Settings › Developer tools and turn on Let AI tools use tug.";

#[derive(Debug)]
pub enum ClientError {
    NotRunning,
    NotSetUp,
    /// tug answered with an error (off, tool off, rate limited, revoked, …).
    Bridge(BridgeError),
    Timeout,
    /// Something answered on the pipe that couldn't prove it was tug.
    Impostor,
    Other(String),
}

impl ClientError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ClientError::Bridge(e) => Some(e.code),
            _ => None,
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => f.write_str(NOT_RUNNING),
            ClientError::NotSetUp => f.write_str(NOT_SET_UP),
            ClientError::Bridge(e) => f.write_str(&e.message),
            ClientError::Timeout => f.write_str("tug didn't answer in time. Try again."),
            ClientError::Impostor => {
                f.write_str("Something other than tug answered. Quit and reopen tug, then try again.")
            }
            ClientError::Other(e) => write!(f, "Couldn't talk to tug: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

fn frame_err(e: FrameError) -> ClientError {
    match e {
        // tug closes after an error it has already sent; a bare close means it went away.
        FrameError::Closed => ClientError::NotRunning,
        other => ClientError::Other(other.to_string()),
    }
}

/// One exchange over an open stream. `token` is asked for only once tug has said it's on.
pub async fn call_over<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    info: &ClientInfo,
    token: impl FnOnce() -> Result<String, ClientError>,
    call: &Call,
) -> Result<serde_json::Value, ClientError> {
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r);
    let client_nonce = auth::new_nonce();
    write_msg(
        &mut w,
        &ClientMsg::Hello {
            v: PROTOCOL_VERSION,
            nonce: client_nonce.clone(),
            client: info.clone(),
        },
    )
    .await
    .map_err(frame_err)?;
    let (server_nonce, server_proof) = match read_msg::<ServerMsg, _>(&mut r, MAX_SERVER_LINE)
        .await
        .map_err(frame_err)?
    {
        ServerMsg::Challenge { v, nonce, proof } if v == PROTOCOL_VERSION => (nonce, proof),
        ServerMsg::Challenge { .. } => {
            return Err(ClientError::Bridge(BridgeError::new(
                ErrorCode::Version,
                crate::server::VERSION_MESSAGE,
            )))
        }
        ServerMsg::Error(e) => return Err(ClientError::Bridge(e)),
        ServerMsg::Result { .. } => return Err(ClientError::Other("unexpected answer".into())),
    };
    let token = token()?;
    if !auth::verify_server(&token, &client_nonce, &server_nonce, &server_proof) {
        // Either an impostor, or tug's token changed since we read it (Revoke access): the
        // caller tells the two apart by looking at the token file again.
        return Err(ClientError::Impostor);
    }
    write_msg(
        &mut w,
        &ClientMsg::Call {
            proof: auth::client_proof(&token, &client_nonce, &server_nonce),
            call: call.clone(),
        },
    )
    .await
    .map_err(frame_err)?;
    match read_msg::<ServerMsg, _>(&mut r, MAX_SERVER_LINE)
        .await
        .map_err(frame_err)?
    {
        ServerMsg::Result { ok } => Ok(ok),
        ServerMsg::Error(e) => Err(ClientError::Bridge(e)),
        ServerMsg::Challenge { .. } => Err(ClientError::Other("unexpected answer".into())),
    }
}

/// A connection to the running tug for this Windows user.
pub struct Client {
    pipe: String,
    token_path: Option<PathBuf>,
    info: ClientInfo,
    /// Read once, then kept: after Revoke access, a long-running MCP server stops working until
    /// it's restarted, which is what revoking means.
    token: Mutex<Option<String>>,
}

impl Client {
    pub fn new(pipe: String, token_path: Option<PathBuf>, info: ClientInfo) -> Client {
        Client {
            pipe,
            token_path,
            info,
            token: Mutex::new(None),
        }
    }

    /// The tug this user runs (or the test build named by `TUG_APP_ID`).
    #[cfg(windows)]
    pub fn for_current_user(info: ClientInfo) -> Result<Client, ClientError> {
        let app_id = crate::paths::client_app_id();
        let sid = crate::win::current_user_sid().map_err(|e| ClientError::Other(e.to_string()))?;
        Ok(Client::new(
            crate::paths::pipe_name(&sid, &app_id),
            crate::paths::token_path(&app_id),
            info,
        ))
    }

    pub fn info(&self) -> &ClientInfo {
        &self.info
    }

    fn token(&self) -> Result<String, ClientError> {
        let mut cached = self.token.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = cached.as_ref() {
            return Ok(t.clone());
        }
        let path = self.token_path.as_ref().ok_or(ClientError::NotSetUp)?;
        let t = match token_file::load(path) {
            Ok(t) => t,
            Err(TokenError::Missing | TokenError::Invalid) => return Err(ClientError::NotSetUp),
            Err(TokenError::Io(e)) => return Err(ClientError::Other(e.to_string())),
        };
        *cached = Some(t.clone());
        Ok(t)
    }

    #[cfg(windows)]
    async fn open(&self) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, ClientError> {
        use tokio::net::windows::named_pipe::ClientOptions;
        const ERROR_FILE_NOT_FOUND: i32 = 2;
        const ERROR_PIPE_BUSY: i32 = 231;
        let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
        loop {
            match ClientOptions::new().open(&self.pipe) {
                Ok(c) => return Ok(c),
                Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => return Err(ClientError::NotRunning),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(ClientError::Timeout);
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => return Err(ClientError::Other(e.to_string())),
            }
        }
    }

    /// Make one call, start to finish within `limit`.
    #[cfg(windows)]
    pub async fn call(&self, call: &Call, limit: Duration) -> Result<serde_json::Value, ClientError> {
        self.call_as_client(&self.info, call, limit).await
    }

    /// `call`, introducing the caller as `info` (the MCP server names each call after the AI
    /// tool that made it).
    #[cfg(windows)]
    pub async fn call_as_client(
        &self,
        info: &ClientInfo,
        call: &Call,
        limit: Duration,
    ) -> Result<serde_json::Value, ClientError> {
        let result = timeout(limit, async {
            let stream = self.open().await?;
            call_over(stream, info, || self.token(), call).await
        })
        .await
        .map_err(|_| ClientError::Timeout)?;
        match result {
            Err(ClientError::Impostor) if self.token_changed() => Err(ClientError::Bridge(BridgeError::new(
                ErrorCode::Unauthorized,
                crate::server::REVOKED_MESSAGE,
            ))),
            other => other,
        }
    }

    /// The token file no longer holds the token this client read: tug's access was revoked
    /// (not an impostor). The kept token stays, so revoking really does lock this client out.
    fn token_changed(&self) -> bool {
        let cached = self.token.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match (cached, self.token_path.as_ref().map(|p| token_file::load(p))) {
            (Some(old), Some(Ok(now))) => old != now,
            _ => false,
        }
    }

    /// `call`, with the answer read as `T`.
    #[cfg(windows)]
    pub async fn call_as<T: DeserializeOwned>(&self, call: &Call, limit: Duration) -> Result<T, ClientError> {
        let v = self.call(call, limit).await?;
        serde_json::from_value(v).map_err(|e| ClientError::Other(format!("unexpected answer: {e}")))
    }
}

/// The usual time limit for a call.
pub fn time_limit(call: &Call) -> Duration {
    match call {
        Call::SendText { .. } => SEND_TEXT_TIMEOUT,
        _ => CALL_TIMEOUT,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::protocol::{ClientKind, Request};
    use crate::server::{handle_connection, BoxFuture, Handler};

    struct Fake {
        enabled: AtomicBool,
        token: Mutex<Option<String>>,
        rejected: AtomicUsize,
        handled: AtomicUsize,
    }

    impl Fake {
        fn new(token: &str) -> Arc<Fake> {
            Arc::new(Fake {
                enabled: AtomicBool::new(true),
                token: Mutex::new(Some(token.into())),
                rejected: AtomicUsize::new(0),
                handled: AtomicUsize::new(0),
            })
        }
    }

    impl Handler for Fake {
        fn enabled(&self) -> bool {
            self.enabled.load(Ordering::SeqCst)
        }
        fn token(&self) -> Option<String> {
            self.token.lock().unwrap().clone()
        }
        fn now_ms(&self) -> i64 {
            1_791_365_400_000
        }
        fn time_limit(&self, _: &Request) -> Duration {
            Duration::from_millis(200)
        }
        fn rejected(&self, _: &ClientInfo, _: ErrorCode) {
            self.rejected.fetch_add(1, Ordering::SeqCst);
        }
        fn handle(
            &self,
            client: ClientInfo,
            request: Request,
        ) -> BoxFuture<'_, Result<serde_json::Value, BridgeError>> {
            self.handled.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                match request {
                    Request::PhoneStatus => Ok(serde_json::json!({"who": client.name})),
                    Request::Media(_) => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        Ok(serde_json::Value::Null)
                    }
                    _ => Err(BridgeError::new(ErrorCode::ToolOff, "off")),
                }
            })
        }
    }

    fn info() -> ClientInfo {
        ClientInfo {
            name: "test-agent".into(),
            kind: ClientKind::Mcp,
        }
    }

    async fn run(server: Arc<Fake>, client_token: Option<&str>, call: Call) -> Result<serde_json::Value, ClientError> {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let s = server.clone();
        let task = tokio::spawn(async move { handle_connection(b, &*s).await });
        let tok = client_token.map(str::to_string);
        let out = call_over(a, &info(), || tok.ok_or(ClientError::NotSetUp), &call).await;
        task.await.unwrap();
        out
    }

    #[tokio::test]
    async fn a_call_with_the_right_token_works() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let v = run(server.clone(), Some(&token), Call::PhoneStatus).await.unwrap();
        assert_eq!(v, serde_json::json!({"who": "test-agent"}));
        assert_eq!(server.handled.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_wrong_token_is_caught_by_the_client_first() {
        let server = Fake::new(&auth::new_token());
        let err = run(server.clone(), Some(&auth::new_token()), Call::PhoneStatus)
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Impostor), "{err:?}");
        assert_eq!(server.handled.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn off_answers_without_asking_for_the_token() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        server.enabled.store(false, Ordering::SeqCst);
        let err = run(server.clone(), None, Call::PhoneStatus).await.unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Off));
        assert!(err.to_string().contains("Developer tools"));
    }

    #[tokio::test]
    async fn bad_arguments_never_reach_the_app() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let call = Call::GetTugboatFile {
            name: "..\\..\\secret".into(),
        };
        let err = run(server.clone(), Some(&token), call).await.unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Invalid));
        assert_eq!(server.handled.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn slow_answers_are_cut_off() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let call = Call::MediaControl { action: "next".into() };
        let err = run(server, Some(&token), call).await.unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::Internal));
    }

    #[tokio::test]
    async fn app_errors_come_back_as_they_are() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let err = run(server, Some(&token), Call::GetLatestCode { copy: false })
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some(ErrorCode::ToolOff));
    }

    /// A client that skips the proof (or forges one) is refused and logged.
    #[tokio::test]
    async fn a_forged_client_proof_is_refused() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let (a, b) = tokio::io::duplex(64 * 1024);
        let s = server.clone();
        let task = tokio::spawn(async move { handle_connection(b, &*s).await });
        let (r, mut w) = tokio::io::split(a);
        let mut r = BufReader::new(r);
        write_msg(
            &mut w,
            &ClientMsg::Hello {
                v: PROTOCOL_VERSION,
                nonce: auth::new_nonce(),
                client: info(),
            },
        )
        .await
        .unwrap();
        let _challenge: ServerMsg = read_msg(&mut r, MAX_SERVER_LINE).await.unwrap();
        write_msg(
            &mut w,
            &ClientMsg::Call {
                proof: "00".repeat(32),
                call: Call::PhoneStatus,
            },
        )
        .await
        .unwrap();
        let answer: ServerMsg = read_msg(&mut r, MAX_SERVER_LINE).await.unwrap();
        task.await.unwrap();
        assert!(matches!(
            answer,
            ServerMsg::Error(BridgeError {
                code: ErrorCode::Unauthorized,
                ..
            })
        ));
        assert_eq!(server.rejected.load(Ordering::SeqCst), 1);
        assert_eq!(server.handled.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn an_old_client_is_told_to_update() {
        let server = Fake::new(&auth::new_token());
        let (a, b) = tokio::io::duplex(4096);
        let s = server.clone();
        let task = tokio::spawn(async move { handle_connection(b, &*s).await });
        let (r, mut w) = tokio::io::split(a);
        let mut r = BufReader::new(r);
        let hello = serde_json::json!({"type": "hello", "v": 99, "nonce": auth::new_nonce(), "client": info()});
        write_msg(&mut w, &hello).await.unwrap();
        let answer: ServerMsg = read_msg(&mut r, MAX_SERVER_LINE).await.unwrap();
        task.await.unwrap();
        assert!(matches!(
            answer,
            ServerMsg::Error(BridgeError {
                code: ErrorCode::Version,
                ..
            })
        ));
    }

    /// The real thing: a named pipe with the user-only ACL, served and called.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn over_a_real_named_pipe() {
        let token = auth::new_token();
        let server = Fake::new(&token);
        let sid = crate::win::current_user_sid().unwrap();
        let pipe = crate::paths::pipe_name(&sid, &format!("tug-bridge-test-{}", std::process::id()));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let h: Arc<dyn Handler> = server.clone();
        let serving = tokio::spawn(crate::server::serve(pipe.clone(), sid, h, async {
            let _ = stop_rx.await;
        }));
        tokio::time::sleep(Duration::from_millis(50)).await;

        // A second server can't take over the name while the first holds it.
        let mut opts = tokio::net::windows::named_pipe::ServerOptions::new();
        opts.first_pipe_instance(true);
        assert!(opts.create(&pipe).is_err());

        let dir = std::env::temp_dir().join(format!("tug-bridge-pipe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token_path = dir.join("bridge.token");
        crate::win::write_private_file(&token_path, token.as_bytes()).unwrap();
        let client = Arc::new(Client::new(pipe.clone(), Some(token_path), info()));
        for _ in 0..3 {
            let v = client.call(&Call::PhoneStatus, CALL_TIMEOUT).await.unwrap();
            assert_eq!(v["who"], "test-agent");
        }
        // More at once than there are connection slots: the rest wait their turn.
        let tasks: Vec<_> = (0..12)
            .map(|_| {
                let c = client.clone();
                tokio::spawn(async move { c.call(&Call::PhoneStatus, CALL_TIMEOUT).await })
            })
            .collect();
        for t in tasks {
            let r = t.await.unwrap();
            assert!(r.is_ok(), "{r:?}");
        }
        // Revoke access: tug and the file get a new token; this client kept the old one, so it's
        // told it was revoked (not that an impostor answered), and stays locked out.
        let rotated = auth::new_token();
        *server.token.lock().unwrap() = Some(rotated.clone());
        crate::win::write_private_file(&dir.join("bridge.token"), rotated.as_bytes()).unwrap();
        for _ in 0..2 {
            let err = client.call(&Call::PhoneStatus, CALL_TIMEOUT).await.unwrap_err();
            assert_eq!(err.code(), Some(ErrorCode::Unauthorized), "{err:?}");
            assert!(err.to_string().contains("Revoke access"));
        }
        // A fresh client (a restarted AI tool) reads the new token and works.
        let fresh = Client::new(pipe.clone(), Some(dir.join("bridge.token")), info());
        assert!(fresh.call(&Call::PhoneStatus, CALL_TIMEOUT).await.is_ok());

        stop_tx.send(()).unwrap();
        serving.await.unwrap().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        // Nobody listening: "tug isn't running".
        let gone = Client::new(pipe, None, info());
        assert!(matches!(
            gone.call(&Call::PhoneStatus, CALL_TIMEOUT).await,
            Err(ClientError::NotRunning)
        ));
    }
}
