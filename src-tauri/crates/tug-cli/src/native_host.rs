//! `tug.exe` as the browser extension's native messaging host: Chrome or Edge starts it with the
//! extension's origin as the first argument and talks to it over stdin/stdout until it closes
//! stdin. Each request is one bridge call, made as `ClientKind::Browser`; the message format and
//! the calls allowed are in `tug_bridge::native`.
//!
//! Requests run side by side (a text waits up to two minutes for the person at tug's card, and
//! the popup's other calls mustn't wait behind it); replies go out one whole message at a time.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tug_bridge::client::{time_limit, Client};
use tug_bridge::native::{parse_request, read_message, BrowserReply, ReadError};
use tug_bridge::protocol::{ClientInfo, ClientKind, BROWSER_EXTENSION};

use crate::format;

pub async fn serve() -> i32 {
    let client = Client::for_current_user(ClientInfo {
        name: BROWSER_EXTENSION.into(),
        kind: ClientKind::Browser,
        launcher: None,
    });

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(bytes) = rx.recv().await {
            if out.write_all(&bytes).await.is_err() || out.flush().await.is_err() {
                return; // The browser went away: nobody to answer.
            }
        }
    });

    let client = client.map(Arc::new);
    let mut calls = JoinSet::new();
    let mut stdin = tokio::io::stdin();
    let code = loop {
        let bytes = match read_message(&mut stdin).await {
            Ok(Some(b)) => b,
            Ok(None) => break format::OK,
            // A malformed stream can't be resynchronized: stop, as the browser would.
            Err(ReadError::TooLong(_)) | Err(ReadError::Io(_)) => break format::FAILED,
        };
        let (id, call) = match parse_request(&bytes) {
            Ok(r) => r,
            Err(reply) => {
                let _ = tx.send(reply.to_bytes());
                continue;
            }
        };
        let client = match &client {
            Ok(c) => Arc::clone(c),
            Err(e) => {
                let _ = tx.send(BrowserReply::failed(id, e).to_bytes());
                continue;
            }
        };
        let tx = tx.clone();
        calls.spawn(async move {
            let reply = match client.call(&call, time_limit(&call)).await {
                Ok(v) => BrowserReply::ok(id, v),
                Err(e) => BrowserReply::failed(id, &e),
            };
            let _ = tx.send(reply.to_bytes());
        });
    };

    // The browser may close stdin while it waits for answers (a one-off message does): finish
    // what's in flight, each within its own time limit, then let the writer drain.
    while calls.join_next().await.is_some() {}
    drop(tx);
    let _ = writer.await;
    code
}
