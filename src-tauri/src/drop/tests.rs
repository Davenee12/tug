//! End-to-end: a Drop server on 127.0.0.1 and a test that plays the phone over a real socket,
//! sealing and signing exactly as the page does.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};

use super::auth;
use super::crypto::{self, ad, Keys};
use super::server;
use super::session::{self, FinishReply, Session, Sink, StateReply, UploadReply, OFFER_CHUNK};
use super::upload::MIN_CHUNK;

#[derive(Default)]
struct TestSink {
    texts: Mutex<Vec<String>>,
}

impl Sink for TestSink {
    fn changed(&self, _urgent: bool) {}
    fn text(&self, text: &str) {
        self.texts.lock().unwrap().push(text.to_string());
    }
}

struct Phone {
    addr: SocketAddr,
    keys: Keys,
    client: String,
    seq: u64,
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    seq: u64,
}

const UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Safari/604.1";

async fn send(addr: SocketAddr, method: &str, path: &str, auth: Option<String>, body: Vec<u8>) -> (u16, Vec<u8>) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", addr.to_string())
        .header("user-agent", UA);
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let res = sender
        .send_request(req.body(Full::new(Bytes::from(body))).unwrap())
        .await
        .unwrap();
    let status = res.status().as_u16();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body)
}

impl Phone {
    fn new(addr: SocketAddr, secret: &[u8], client: &str) -> Phone {
        // Like the page: sequence numbers start from the clock and only go up.
        Phone {
            addr,
            keys: Keys::derive(secret),
            client: client.into(),
            seq: 1_790_000_000_000 * 1024,
        }
    }

    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    async fn raw(&mut self, method: &str, path: &str, body: Vec<u8>) -> Reply {
        let seq = self.next();
        let header = auth::header(&self.keys, &self.client, seq, method, path);
        let (status, body) = send(self.addr, method, path, Some(header), body).await;
        Reply { status, body, seq }
    }

    /// A JSON call: the body sealed to this request, the reply opened against it.
    async fn call<T: DeserializeOwned>(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, Option<T>) {
        let seq = self.seq + 1;
        let sealed = body
            .map(|b| {
                self.keys
                    .seal(&ad::request(&self.client, seq), &serde_json::to_vec(&b).unwrap())
            })
            .unwrap_or_default();
        let r = self.raw(method, path, sealed).await;
        assert_eq!(r.seq, seq);
        if r.status != 200 {
            return (r.status, None);
        }
        let plain = self
            .keys
            .open(&ad::response(&self.client, r.seq), &r.body)
            .expect("reply opens");
        (r.status, Some(serde_json::from_slice(&plain).unwrap()))
    }

    async fn put_chunk(&mut self, file_id: &str, index: u32, data: &[u8]) -> u16 {
        let sealed = self.keys.seal(&ad::up(file_id, index), data);
        self.raw("PUT", &format!("/api/up/{file_id}/{index}"), sealed)
            .await
            .status
    }
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tug-drop-test-{}", crypto::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

#[tokio::test]
async fn phone_session_over_a_socket() {
    let root = temp_dir();
    let folder = root.join("tug Drop");
    let secret = crypto::new_secret();
    let sink = Arc::new(TestSink::default());
    let session = Arc::new(Session::new(&secret, folder.clone(), sink.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(server::serve(listener, session.clone(), async {
        let _ = stop_rx.await;
    }));

    // The page itself needs no secret (the fragment never reaches the server).
    let (status, html) = send(addr, "GET", "/", None, Vec::new()).await;
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&html).contains("tug"));

    // No signature, or the wrong secret: refused, and nothing gets bound.
    assert_eq!(send(addr, "GET", "/api/state", None, Vec::new()).await.0, 401);
    let mut stranger = Phone::new(addr, &[0u8; 16], "strangerAAAAAAAAAAAAAA");
    assert_eq!(stranger.raw("GET", "/api/state", Vec::new()).await.status, 401);
    assert!(session.snapshot().phone.is_none());

    // The phone that scanned the code binds the session.
    let mut phone = Phone::new(addr, &secret, "phoneAAAAAAAAAAAAAAAAA");
    let (status, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        state.unwrap(),
        StateReply {
            offers: vec![],
            text: None
        }
    );
    assert_eq!(session.snapshot().phone.as_deref(), Some("iPhone"));

    // A second phone with the same code is turned away.
    let mut second = Phone::new(addr, &secret, "secondBBBBBBBBBBBBBBBB");
    assert_eq!(second.raw("GET", "/api/state", Vec::new()).await.status, 403);

    // A replayed request (same signature, same sequence number) is refused.
    let seq = phone.next();
    let header = auth::header(&phone.keys, &phone.client, seq, "GET", "/api/state");
    assert_eq!(
        send(addr, "GET", "/api/state", Some(header.clone()), Vec::new())
            .await
            .0,
        200
    );
    assert_eq!(send(addr, "GET", "/api/state", Some(header), Vec::new()).await.0, 401);

    // --- Upload a multi-chunk file, interrupted and resumed ---
    let chunk = MIN_CHUNK as usize;
    let data = pattern(5 * chunk + 1000, 7);
    let file_id = "fileAAAAAAAAAAAAAAAAAA";
    let begin = json!({ "name": "../../IMG_0001.HEIC", "size": data.len(), "chunkSize": MIN_CHUNK });
    let (status, reply) = phone
        .call::<UploadReply>("POST", &format!("/api/up/{file_id}"), Some(begin.clone()))
        .await;
    assert_eq!(status, 200);
    let reply = reply.unwrap();
    assert_eq!(reply.chunks, 6);
    assert!(reply.received.is_empty());
    let piece = |i: usize| &data[i * chunk..((i + 1) * chunk).min(data.len())];
    for i in 0..3 {
        assert_eq!(phone.put_chunk(file_id, i as u32, piece(i)).await, 204);
    }
    // The connection drops mid-chunk: chunk 3 arrives mangled, chunk 4 never does.
    let mut mangled = phone.keys.seal(&ad::up(file_id, 3), piece(3));
    mangled[40] ^= 0xff;
    assert_eq!(
        phone.raw("PUT", &format!("/api/up/{file_id}/3"), mangled).await.status,
        400
    );
    // A chunk sealed for another index can't be slotted in either.
    let moved = phone.keys.seal(&ad::up(file_id, 5), piece(4));
    assert_eq!(
        phone.raw("PUT", &format!("/api/up/{file_id}/4"), moved).await.status,
        400
    );
    // Finishing now is refused: not everything is in.
    assert_eq!(
        phone
            .call::<FinishReply>("POST", &format!("/api/finish/{file_id}"), None)
            .await
            .0,
        409
    );
    let progress = session.snapshot().incoming[0].clone();
    assert_eq!(progress.received, 3 * chunk as u64);
    assert!(!progress.done);

    // Safari comes back: the page asks what arrived and sends the rest.
    let (_, reply) = phone
        .call::<UploadReply>("POST", &format!("/api/up/{file_id}"), Some(begin.clone()))
        .await;
    assert_eq!(reply.unwrap().received, vec![[0, 3]]);
    for i in 3..6 {
        assert_eq!(phone.put_chunk(file_id, i as u32, piece(i)).await, 204);
    }
    let (status, done) = phone
        .call::<FinishReply>("POST", &format!("/api/finish/{file_id}"), None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(done.unwrap().saved_as, "IMG_0001.HEIC");
    assert_eq!(std::fs::read(folder.join("IMG_0001.HEIC")).unwrap(), data);
    assert!(session.snapshot().incoming[0].done);
    // A finish whose reply was lost is answered the same way; asking again reports it saved.
    let (_, again) = phone
        .call::<FinishReply>("POST", &format!("/api/finish/{file_id}"), None)
        .await;
    assert_eq!(again.unwrap().saved_as, "IMG_0001.HEIC");
    let (_, reply) = phone
        .call::<UploadReply>("POST", &format!("/api/up/{file_id}"), Some(begin))
        .await;
    assert_eq!(reply.unwrap().saved_as.as_deref(), Some("IMG_0001.HEIC"));

    // The same name again gets " (2)", never overwriting the first.
    let small = pattern(10, 1);
    let second_id = "fileBBBBBBBBBBBBBBBBBB";
    let begin = json!({ "name": "IMG_0001.HEIC", "size": small.len(), "chunkSize": MIN_CHUNK });
    phone
        .call::<UploadReply>("POST", &format!("/api/up/{second_id}"), Some(begin))
        .await;
    assert_eq!(phone.put_chunk(second_id, 0, &small).await, 204);
    let (_, done) = phone
        .call::<FinishReply>("POST", &format!("/api/finish/{second_id}"), None)
        .await;
    assert_eq!(done.unwrap().saved_as, "IMG_0001 (2).HEIC");
    assert_eq!(std::fs::read(folder.join("IMG_0001.HEIC")).unwrap(), data);
    assert_eq!(session.sending(), 0);

    // Too big for one file: refused up front.
    let huge = json!({ "name": "big.mov", "size": 9u64 << 30, "chunkSize": MIN_CHUNK });
    assert_eq!(
        phone
            .call::<UploadReply>("POST", "/api/up/fileCCCCCCCCCCCCCCCCCC", Some(huge))
            .await
            .0,
        413
    );

    // --- Text from the phone lands on the clipboard (via the sink) and in the panel ---
    let (status, _) = phone
        .call::<Value>("POST", "/api/text", Some(json!({ "text": "hello from the phone" })))
        .await;
    assert_eq!(status, 200);
    assert_eq!(*sink.texts.lock().unwrap(), vec!["hello from the phone".to_string()]);
    assert_eq!(session.snapshot().texts[0].text, "hello from the phone");

    // --- A file offered on the PC, downloaded and decrypted by the phone ---
    let offered = pattern(OFFER_CHUNK as usize + 12345, 3);
    let offered_path = root.join("report.pdf");
    std::fs::write(&offered_path, &offered).unwrap();
    assert!(
        session.offer(&[offered_path.clone(), root.clone()]).len() == 1,
        "the folder is skipped"
    );
    session.set_pc_text("from the pc").unwrap();
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    let state = state.unwrap();
    assert_eq!(state.text.unwrap().text, "from the pc");
    let offer = &state.offers[0];
    assert_eq!(
        (offer.name.as_str(), offer.size, offer.chunks),
        ("report.pdf", offered.len() as u64, 2)
    );
    assert_eq!(offer.mime, "application/pdf");
    let mut got = Vec::new();
    for i in 0..offer.chunks {
        let r = phone
            .raw("GET", &format!("/api/down/{}/{i}", offer.id), Vec::new())
            .await;
        assert_eq!(r.status, 200);
        // Sealed for "down", this offer and this index: nothing else opens it.
        assert!(phone.keys.open(&ad::up(&offer.id, i), &r.body).is_none());
        got.extend(phone.keys.open(&ad::down(&offer.id, i), &r.body).expect("chunk opens"));
    }
    assert_eq!(got, offered);
    assert_eq!(session.snapshot().outgoing[0].downloads, 1);

    // --- Closing ends the session for good, even on a live connection ---
    session.close();
    assert_eq!(phone.raw("GET", "/api/state", Vec::new()).await.status, 410);
    assert_eq!(send(addr, "GET", "/", None, Vec::new()).await.0, 410);
    let _ = stop_tx.send(());
    server.await.unwrap();

    // Nothing unfinished is left behind.
    session::clean_incoming(&folder);
    assert!(!folder.join(".incoming").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
