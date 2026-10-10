//! End-to-end: a Tugboat server on 127.0.0.1 and a test that plays the phone over a real socket,
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
use super::pad::{PadEvent, PadReply, BURST};
use super::server;
use super::session::{self, FinishReply, Session, Sink, StateReply, UploadReply, OFFER_CHUNK};
use super::upload::MIN_CHUNK;

#[derive(Default)]
struct TestSink {
    texts: Mutex<Vec<String>>,
    pads: Mutex<Vec<PadEvent>>,
}

impl Sink for TestSink {
    fn changed(&self, _urgent: bool) {}
    fn text(&self, text: &str) {
        self.texts.lock().unwrap().push(text.to_string());
    }
    fn pad(&self, event: PadEvent) {
        self.pads.lock().unwrap().push(event);
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
    let dir = std::env::temp_dir().join(format!("tugboat-test-{}", crypto::new_id()));
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
    let folder = root.join("Tugboat");
    let secret = crypto::new_secret();
    let sink = Arc::new(TestSink::default());
    let incoming = root.join("incoming");
    let session = Arc::new(Session::new(&secret, folder.clone(), incoming.clone(), sink.clone()));
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
            text: None,
            game: false
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
    assert_eq!(send(addr, "GET", "/api/state", Some(header), Vec::new()).await.0, 409);

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
    // Marked as downloaded from the internet, like a browser download (SmartScreen, Protected View).
    #[cfg(windows)]
    {
        let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", folder.join("IMG_0001.HEIC").display()));
        assert!(zone.unwrap().contains("ZoneId=3"));
    }
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
    assert_eq!(session.uploads_in_progress(), 0);

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
    assert!(
        std::fs::read_dir(&incoming).unwrap().next().is_none(),
        "saved files left the incoming folder"
    );
    session::clean_incoming(&incoming);
    assert!(!incoming.exists());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_link_never_carries_a_client_id() {
    use std::net::Ipv4Addr;
    let secret = [0u8; 16];
    let ip = Ipv4Addr::new(192, 168, 1, 20);
    assert_eq!(
        super::link(ip, 53211, &secret),
        "http://192.168.1.20:53211/#AAAAAAAAAAAAAAAAAAAAAA"
    );
}

#[tokio::test]
async fn after_unbind_a_new_phone_binds_and_the_old_one_is_refused() {
    let root = temp_dir();
    let secret = crypto::new_secret();
    let session = Arc::new(Session::new(
        &secret,
        root.join("Tugboat"),
        root.join("incoming"),
        Arc::new(TestSink::default()),
    ));
    let (addr, stop, server) = start_server(session.clone(), server::Limits::default()).await;
    let mut first = Phone::new(addr, &secret, "firstAAAAAAAAAAAAAAAAA");
    assert_eq!(first.raw("GET", "/api/state", Vec::new()).await.status, 200);
    let old_seq = first.seq;
    // The address changed (a new browser origin): the next phone to scan binds.
    session.unbind();
    assert!(session.snapshot().phone.is_none(), "the panel goes back to waiting");
    let mut second = Phone::new(addr, &secret, "secondAAAAAAAAAAAAAAAA");
    second.seq = first.seq + 1000;
    assert_eq!(second.raw("GET", "/api/state", Vec::new()).await.status, 200);
    assert_eq!(first.raw("GET", "/api/state", Vec::new()).await.status, 403);
    // A sequence number from before the unbind is still a replay.
    let header = auth::header(&second.keys, &second.client, old_seq, "GET", "/api/state");
    assert_eq!(send(addr, "GET", "/api/state", Some(header), Vec::new()).await.0, 409);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn slow_or_excess_connections_are_closed() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = temp_dir();
    let session = Arc::new(Session::new(
        &crypto::new_secret(),
        root.join("Tugboat"),
        root.join("incoming"),
        Arc::new(TestSink::default()),
    ));
    let limits = server::Limits {
        header_timeout: std::time::Duration::from_millis(300),
        body_timeout: std::time::Duration::from_millis(300),
        max_connections: 2,
    };
    let (addr, stop, server) = start_server(session, limits).await;
    let closed_within = |mut s: TcpStream, secs: u64| async move {
        let mut buf = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(secs), s.read_to_end(&mut buf))
            .await
            .is_ok()
    };
    // Half a request head, then nothing (slowloris): closed once the header timeout passes.
    let mut slow = TcpStream::connect(addr).await.unwrap();
    slow.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").await.unwrap();
    // Fill the other slot, then a third connection is turned away at once.
    let mut second = TcpStream::connect(addr).await.unwrap();
    second.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let third = TcpStream::connect(addr).await.unwrap();
    assert!(closed_within(third, 1).await, "over the cap: closed straight away");
    assert!(
        closed_within(slow, 3).await,
        "a stalled head is closed by the header timeout"
    );
    assert!(closed_within(second, 3).await);
    // The server still answers afterwards.
    assert_eq!(send(addr, "GET", "/", None, Vec::new()).await.0, 200);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// The game controller channel, over a real socket: only the bound phone, only while the game
/// asks, only steering, never faster than a phone sends, and it touches nothing else.
#[tokio::test]
async fn game_controller_inputs_only_steer_the_game() {
    let root = temp_dir();
    let secret = crypto::new_secret();
    let sink = Arc::new(TestSink::default());
    let session = Arc::new(Session::new(
        &secret,
        root.join("Tugboat"),
        root.join("incoming"),
        sink.clone(),
    ));
    let (addr, stop, server) = start_server(session.clone(), server::Limits::default()).await;
    let mut phone = Phone::new(addr, &secret, "phoneAAAAAAAAAAAAAAAAA");
    let input = |steer: i64, boost: bool| Some(json!({ "steer": steer, "boost": boost }));

    // The phone binds the session the usual way; the game hasn't asked for a controller yet.
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(!state.unwrap().game);
    assert_eq!(
        phone.call::<PadReply>("POST", "/api/pad", input(50, false)).await.0,
        409
    );
    assert!(sink.pads.lock().unwrap().is_empty());

    // The game opens the channel: the page sees it at its next check-in.
    assert!(session.pad_open());
    assert!(!session.pad_open(), "opening twice is one channel");
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(state.unwrap().game);

    // A steering input reaches the game; the reply carries what the phone shows.
    session.pad_feedback(PadReply { paused: true, hits: 2 });
    let (status, reply) = phone.call::<PadReply>("POST", "/api/pad", input(-40, true)).await;
    assert_eq!(status, 200);
    assert_eq!(reply.unwrap(), PadReply { paused: true, hits: 2 });
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert!(last.connected && last.boost && (last.steer + 0.4).abs() < 1e-6);
    // A heartbeat with the same state doesn't bother the game again.
    let before = sink.pads.lock().unwrap().len();
    assert_eq!(
        phone.call::<PadReply>("POST", "/api/pad", input(-40, true)).await.0,
        200
    );
    assert_eq!(sink.pads.lock().unwrap().len(), before);

    // Not signed, signed with another secret, or another device: refused, game untouched.
    assert_eq!(send(addr, "POST", "/api/pad", None, Vec::new()).await.0, 401);
    let mut stranger = Phone::new(addr, &[1u8; 16], "strangerAAAAAAAAAAAAAA");
    assert_eq!(
        stranger.call::<PadReply>("POST", "/api/pad", input(100, false)).await.0,
        401
    );
    let mut second = Phone::new(addr, &secret, "secondBBBBBBBBBBBBBBBB");
    assert_eq!(
        second.call::<PadReply>("POST", "/api/pad", input(100, false)).await.0,
        403
    );
    // A replayed input (same signature, same body) is refused.
    let seq = phone.next();
    let header = auth::header(&phone.keys, &phone.client, seq, "POST", "/api/pad");
    let body = phone
        .keys
        .seal(&ad::request(&phone.client, seq), br#"{"steer":100,"boost":false}"#);
    assert_eq!(
        send(addr, "POST", "/api/pad", Some(header.clone()), body.clone())
            .await
            .0,
        200
    );
    assert_eq!(send(addr, "POST", "/api/pad", Some(header), body).await.0, 409);
    // A body sealed to another request (lifted from elsewhere) doesn't open.
    let lifted = phone
        .keys
        .seal(&ad::request(&phone.client, 1), br#"{"steer":0,"boost":false}"#);
    assert_eq!(phone.raw("POST", "/api/pad", lifted).await.status, 400);
    // Anything but steering is refused: extra fields, out of range, oversized.
    assert_eq!(
        phone
            .call::<PadReply>(
                "POST",
                "/api/pad",
                Some(json!({ "steer": 0, "boost": false, "text": "hi" }))
            )
            .await
            .0,
        400
    );
    assert_eq!(
        phone.call::<PadReply>("POST", "/api/pad", input(101, false)).await.0,
        400
    );
    assert_eq!(phone.raw("POST", "/api/pad", vec![0u8; 4096]).await.status, 413);
    let steering = *sink.pads.lock().unwrap().last().unwrap();
    assert_eq!(steering.steer, 1.0, "only the one good input since moved the boat");

    // Inputs never reached anything else in the session.
    let snap = session.snapshot();
    assert!(snap.texts.is_empty() && snap.incoming.is_empty() && snap.outgoing.is_empty());
    assert!(sink.texts.lock().unwrap().is_empty());

    // A flood is cut off with "busy" (the phone sends at most ~30 a second). Drained here rather
    // than over the socket, so a slow test machine can't refill the bucket mid-flood.
    let refused = (0..(BURST as usize + 5))
        .filter(|_| session.pad_admit().is_err())
        .count();
    assert!(refused >= 5, "a flood hits the rate limit");
    let mut saw_busy = false;
    for _ in 0..10 {
        while session.pad_admit().is_ok() {}
        if phone.call::<PadReply>("POST", "/api/pad", input(10, false)).await.0 == 429 {
            saw_busy = true;
            break;
        }
    }
    assert!(saw_busy, "the phone is told it's going too fast");

    // Silence: the game hears the phone went.
    tokio::time::sleep(super::pad::QUIET + std::time::Duration::from_millis(50)).await;
    assert!(session.pad_tick());
    let gone = *sink.pads.lock().unwrap().last().unwrap();
    assert!(!gone.connected && gone.steer == 0.0);

    // The game closes: inputs are refused again and the page leaves controller mode.
    session.pad_close();
    assert!(!session.pad_tick(), "the watch stops with the channel");
    assert_eq!(phone.call::<PadReply>("POST", "/api/pad", input(0, false)).await.0, 409);
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(!state.unwrap().game);

    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// Closing the channel while a phone is steering tells the game at once.
#[test]
fn closing_the_controller_tells_the_game_the_phone_is_gone() {
    let root = temp_dir();
    let secret = crypto::new_secret();
    let sink = Arc::new(TestSink::default());
    let session = Session::new(&secret, root.join("Tugboat"), root.join("incoming"), sink.clone());
    let keys = Keys::derive(&secret);
    let client = "phoneAAAAAAAAAAAAAAAAA";
    let seq = 1_790_000_000_000 * 1024;
    let header = auth::header(&keys, client, seq, "POST", "/api/pad");
    let ok = session.authorize(Some(&header), "POST", "/api/pad", None).unwrap();
    session.pad_open();
    session.pad_admit().unwrap();
    let body = keys.seal(&ad::request(client, seq), br#"{"steer":-100,"boost":false}"#);
    session.pad_input(&ok, &body).unwrap();
    assert!(sink.pads.lock().unwrap().last().unwrap().connected);
    session.pad_close();
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert!(!last.connected && last.steer == 0.0);
    std::fs::remove_dir_all(&root).ok();
}

/// Start a server on 127.0.0.1 with the given limits; returns its address, a stop switch and the task.
async fn start_server(
    session: Arc<Session>,
    limits: server::Limits,
) -> (
    SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(server::serve_with(
        listener,
        session,
        async {
            let _ = rx.await;
        },
        limits,
    ));
    (addr, tx, task)
}

/// Not a test: serves the real built page (`npm run build:tugboat` first) on 127.0.0.1 for a few
/// minutes, so it can be driven from a desktop browser against this server. Prints the link.
///   cargo test --manifest-path src-tauri/Cargo.toml manual_page -- --ignored --nocapture
/// Received files land in a temp folder (printed); one file is on offer; TUGBOAT_SECS sets how long.
/// TUGBOAT_GAME=1 also opens the game controller channel, and prints each controller event.
#[tokio::test]
#[ignore]
async fn manual_page() {
    let root = temp_dir();
    let folder = root.join("Tugboat");
    let secret = crypto::new_secret();
    let sink = Arc::new(TestSink::default());
    let session = Arc::new(Session::new(
        &secret,
        folder.clone(),
        root.join("incoming"),
        sink.clone(),
    ));
    let offered = root.join("Offered from PC.txt");
    std::fs::write(&offered, "Hello from tug on the PC.\n".repeat(200_000)).unwrap();
    session.offer(&[offered]);
    session.set_pc_text("Text from the PC").unwrap();
    if std::env::var("TUGBOAT_GAME").is_ok() {
        session.pad_open();
        let watched = session.clone();
        let pads = sink.clone();
        tokio::spawn(async move {
            let mut shown = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                watched.pad_tick();
                let events = pads.pads.lock().unwrap().clone();
                for e in &events[shown..] {
                    println!("controller: {e:?}");
                }
                shown = events.len();
            }
        });
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    println!("Tugboat page: http://{addr}/#{}", crypto::b64(&secret));
    println!("received files: {}", folder.display());
    let secs = std::env::var("TUGBOAT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let watcher = session.clone();
    tokio::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let s = watcher.snapshot();
            let line = format!(
                "phone={:?} incoming={:?} texts={:?} downloads={:?}",
                s.phone,
                s.incoming
                    .iter()
                    .map(|i| (&i.name, i.received, i.size, i.done))
                    .collect::<Vec<_>>(),
                s.texts.iter().map(|t| &t.text).collect::<Vec<_>>(),
                s.outgoing.iter().map(|o| o.downloads).collect::<Vec<_>>()
            );
            if line != last {
                println!("{line}");
                last = line;
            }
        }
    });
    server::serve(
        listener,
        session,
        tokio::time::sleep(std::time::Duration::from_secs(secs)),
    )
    .await;
}
