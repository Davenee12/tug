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
use super::auth::Authorized;
use super::clip::{ClipEvent, ClipKind, ClipState, Prepared};
use super::crypto::{self, ad, Keys};
use super::pad::{PadEvent, PadReply, BURST, FIRST, SECOND};
use super::server;
use super::session::{self, FinishReply, PageClip, Session, Sink, StateReply, UploadReply, OFFER_CHUNK};
use super::upload::MIN_CHUNK;

#[derive(Default)]
struct TestSink {
    texts: Mutex<Vec<String>>,
    pads: Mutex<Vec<PadEvent>>,
    clips: Mutex<Vec<ClipEvent>>,
}

impl Sink for TestSink {
    fn changed(&self, _urgent: bool) {}
    fn text(&self, text: &str) {
        self.texts.lock().unwrap().push(text.to_string());
    }
    fn pad(&self, event: PadEvent) {
        self.pads.lock().unwrap().push(event);
    }
    fn clip(&self, event: ClipEvent) {
        self.clips.lock().unwrap().push(event);
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

    /// A phone that scanned the Player 2 code: it holds only the guest secret, never the session's.
    fn guest(addr: SocketAddr, secret: &[u8], client: &str) -> Phone {
        Phone::new(addr, &crypto::guest_secret(secret), client)
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
            game: false,
            clip: None,
            guest: false
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

#[test]
fn the_player_2_link_carries_the_guest_secret_never_the_session_secret() {
    use std::net::Ipv4Addr;
    let secret = crypto::new_secret();
    let ip = Ipv4Addr::new(192, 168, 1, 20);
    let link = super::guest_link(ip, 53211, &secret);
    let guest = crypto::b64(&crypto::guest_secret(&secret));
    assert_eq!(link, format!("http://192.168.1.20:53211/#g.{guest}"));
    assert!(!link.contains(&crypto::b64(&secret)));
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
    // Fill the other slot. A third connection makes room by closing the oldest that never
    // proved itself (the stalled one), at once rather than after its timeout.
    let mut second = TcpStream::connect(addr).await.unwrap();
    second.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let third = TcpStream::connect(addr).await.unwrap();
    assert!(
        closed_within(slow, 1).await,
        "over the cap: the oldest unproven connection is closed"
    );
    assert!(
        closed_within(second, 3).await,
        "a stalled head is closed by the header timeout"
    );
    assert!(closed_within(third, 3).await);
    // The server still answers afterwards.
    assert_eq!(send(addr, "GET", "/", None, Vec::new()).await.0, 200);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// A device without the secret gets one answer per connection (no idling on keep-alive), and
/// can't fill every slot: the phone still gets in, and its kept-alive connection stays open.
#[tokio::test]
async fn strangers_cant_hold_connections_or_lock_the_phone_out() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = temp_dir();
    let secret = crypto::new_secret();
    let session = Arc::new(Session::new(
        &secret,
        root.join("Tugboat"),
        root.join("incoming"),
        Arc::new(TestSink::default()),
    ));
    let limits = server::Limits {
        max_connections: 3,
        ..server::Limits::default()
    };
    let (addr, stop, server) = start_server(session.clone(), limits).await;

    // An unauthenticated request on a keep-alive connection: answered, then the server hangs up.
    let mut stranger = TcpStream::connect(addr).await.unwrap();
    stranger
        .write_all(b"GET /api/state HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n")
        .await
        .unwrap();
    let mut reply = Vec::new();
    let read = tokio::time::timeout(std::time::Duration::from_secs(3), stranger.read_to_end(&mut reply)).await;
    assert!(read.is_ok(), "the connection was closed after the refusal");
    let reply = String::from_utf8_lossy(&reply).to_lowercase();
    assert!(reply.starts_with("http/1.1 401"), "{reply}");
    assert!(reply.contains("connection: close"), "{reply}");

    // Strangers fill every slot with connections that send nothing.
    let mut idle = Vec::new();
    for _ in 0..3 {
        idle.push(TcpStream::connect(addr).await.unwrap());
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // The phone still gets in, and its connection carries request after request.
    let keys = Keys::derive(&secret);
    let client = "phoneAAAAAAAAAAAAAAAAA";
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let conn = tokio::spawn(conn);
    let mut seq = 1_790_000_000_000u64 * 1024;
    for _ in 0..3 {
        seq += 1;
        let req = Request::builder()
            .method("GET")
            .uri("/api/state")
            .header("host", addr.to_string())
            .header("authorization", auth::header(&keys, client, seq, "GET", "/api/state"))
            .body(Full::new(Bytes::new()))
            .unwrap();
        sender.ready().await.expect("the phone's connection stays open");
        let res = sender.send_request(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        assert!(res.headers().get("connection").is_none());
        res.into_body().collect().await.unwrap();
    }
    // More strangers arrive: they push out other strangers, never the phone.
    for _ in 0..5 {
        idle.push(TcpStream::connect(addr).await.unwrap());
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    seq += 1;
    let req = Request::builder()
        .method("GET")
        .uri("/api/state")
        .header("host", addr.to_string())
        .header("authorization", auth::header(&keys, client, seq, "GET", "/api/state"))
        .body(Full::new(Bytes::new()))
        .unwrap();
    sender.ready().await.expect("still open");
    assert_eq!(sender.send_request(req).await.unwrap().status().as_u16(), 200);
    assert!(!conn.is_finished());

    drop(sender);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).ok();
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
    assert!(session.pad_open(1));
    assert!(!session.pad_open(1), "opening twice is one channel");
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(state.unwrap().game);

    // A steering input reaches the game; the reply carries what the phone shows.
    let feedback = PadReply {
        paused: true,
        hits: 2,
        player: None,
    };
    session.pad_feedback(FIRST, feedback);
    let (status, reply) = phone.call::<PadReply>("POST", "/api/pad", input(-40, true)).await;
    assert_eq!(status, 200);
    assert_eq!(reply.unwrap(), feedback);
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert!(last.connected && last.boost && last.seat == FIRST && (last.steer + 0.4).abs() < 1e-6);
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
    let as_phone = Authorized {
        client: phone.client.clone(),
        seq: 0,
        first: false,
        guest: false,
    };
    let refused = (0..(BURST as usize + 5))
        .filter(|_| session.pad_admit(&as_phone).is_err())
        .count();
    assert!(refused >= 5, "a flood hits the rate limit");
    let mut saw_busy = false;
    for _ in 0..10 {
        while session.pad_admit(&as_phone).is_ok() {}
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

/// The error code in a refusal's body (`{"error":"full"}` → "full").
fn code(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body).unwrap()["error"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// A two-player game, over a real socket: exactly two phones may steer, the first with the
/// session's keys and the second with the Player 2 code's, each in its own seat; a third is turned
/// away; the second phone can only steer (never files, text or the panel); and going back to one
/// player lets it go.
#[tokio::test]
async fn a_two_player_game_takes_exactly_two_controllers() {
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
    let input = |steer: i64, boost: bool| Some(json!({ "steer": steer, "boost": boost }));
    let mut first = Phone::new(addr, &secret, "firstAAAAAAAAAAAAAAAAA");
    let mut second = Phone::guest(addr, &secret, "secondBBBBBBBBBBBBBBBB");
    // The second phone's clock is a few minutes behind the first's: its own replay window.
    second.seq -= 5 * 60_000 * 1024;
    let mut third = Phone::guest(addr, &secret, "thirdCCCCCCCCCCCCCCCCC");
    session.set_pc_text("only for the first phone").unwrap();

    // The first phone binds the session; a one-player game has no seat for the Player 2 code.
    assert_eq!(first.raw("GET", "/api/state", Vec::new()).await.status, 200);
    session.pad_open(1);
    assert!(!session.wants_guest(), "no Player 2 code to show");
    let r = second.raw("GET", "/api/state", Vec::new()).await;
    assert_eq!((r.status, code(&r.body).as_str()), (403, "no-race"));

    // Two players: the second phone joins, sees only that there's a race, and steers seat 2.
    assert!(!session.pad_open(2), "the same channel, now for two");
    assert!(session.wants_guest(), "the Player 2 code is shown");
    let (status, state) = second.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        state.unwrap(),
        StateReply {
            offers: vec![],
            text: None,
            game: true,
            clip: None,
            guest: true
        }
    );
    let feedback = |player| PadReply {
        paused: false,
        hits: 0,
        player: Some(player),
    };
    session.pad_feedback(FIRST, feedback(2));
    session.pad_feedback(SECOND, feedback(1));
    let (status, reply) = second.call::<PadReply>("POST", "/api/pad", input(70, true)).await;
    assert_eq!((status, reply.unwrap().player), (200, Some(1)));
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert_eq!((last.seat, last.connected, last.boost), (SECOND, true, true));
    assert!((last.steer - 0.7).abs() < 1e-6);
    // The first phone is seat 1, and its reply is its own.
    let (_, reply) = first.call::<PadReply>("POST", "/api/pad", input(-20, false)).await;
    assert_eq!(reply.unwrap().player, Some(2));
    assert_eq!(sink.pads.lock().unwrap().last().unwrap().seat, FIRST);
    // The first phone still sees the session as its own.
    let (_, state) = first.call::<StateReply>("GET", "/api/state", None).await;
    let state = state.unwrap();
    assert!(state.game && !state.guest);
    assert_eq!(state.text.unwrap().text, "only for the first phone");

    // A third phone with the Player 2 code is turned away from the race.
    let r = third.raw("GET", "/api/state", Vec::new()).await;
    assert_eq!((r.status, code(&r.body).as_str()), (403, "full"));
    assert_eq!(
        third.call::<PadReply>("POST", "/api/pad", input(100, false)).await.0,
        403
    );
    // One that scanned Tugboat's own code is told to scan the Player 2 code: it never becomes the
    // guest, and never gets in.
    let mut wrong_code = Phone::new(addr, &secret, "fourthDDDDDDDDDDDDDDDD");
    let r = wrong_code.raw("GET", "/api/state", Vec::new()).await;
    assert_eq!((r.status, code(&r.body).as_str()), (403, "player-2-code"));

    // The second phone can only steer: no text, no files, nothing from the PC.
    assert_eq!(
        second
            .call::<Value>("POST", "/api/text", Some(json!({ "text": "hi" })))
            .await
            .0,
        403
    );
    let begin = json!({ "name": "a.jpg", "size": 10, "chunkSize": MIN_CHUNK });
    assert_eq!(
        second
            .call::<UploadReply>("POST", "/api/up/fileAAAAAAAAAAAAAAAAAA", Some(begin))
            .await
            .0,
        403
    );
    assert_eq!(second.raw("GET", "/api/down/x/0", Vec::new()).await.status, 403);
    let snap = session.snapshot();
    assert!(snap.texts.is_empty() && snap.incoming.is_empty());
    assert_eq!(
        snap.phone.as_deref(),
        Some("iPhone"),
        "the panel's phone is still the first"
    );

    // Back to one player: seat 2's phone is let go (the game hears it's gone), and it waits.
    session.pad_open(1);
    assert_eq!(*sink.pads.lock().unwrap().last().unwrap(), PadEvent::gone(SECOND));
    assert_eq!(
        second.call::<PadReply>("POST", "/api/pad", input(0, false)).await.0,
        409
    );
    let (_, state) = second.call::<StateReply>("GET", "/api/state", None).await;
    let state = state.unwrap();
    assert!(!state.game && state.guest);
    // A rematch for two: the same second phone steers again.
    session.pad_open(2);
    assert_eq!(
        second.call::<PadReply>("POST", "/api/pad", input(10, false)).await.0,
        200
    );

    // The game closes: both phones are gone at once.
    let before = sink.pads.lock().unwrap().len();
    session.pad_close();
    let pads = sink.pads.lock().unwrap()[before..].to_vec();
    assert_eq!(pads, vec![PadEvent::gone(FIRST), PadEvent::gone(SECOND)]);
    assert_eq!(
        second.call::<PadReply>("POST", "/api/pad", input(0, false)).await.0,
        409
    );

    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// The Player 2 code over a real socket: a phone holding only the guest secret joins a race as
/// Player 2, even as the very first phone to connect, but never becomes the session's phone and
/// can't reach files, text, the clipboard or uploads. The owner still binds with the session secret.
#[tokio::test]
async fn a_player_2_code_only_steers_and_never_owns_the_session() {
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
    let input = |steer: i64, boost: bool| Some(json!({ "steer": steer, "boost": boost }));
    let mut guest = Phone::guest(addr, &secret, "guestAAAAAAAAAAAAAAAAA");
    session.set_pc_text("only for the owner").unwrap();
    let offered = root.join("report.pdf");
    std::fs::write(&offered, b"for the owner only").unwrap();
    session.offer(&[offered]);
    let offer_id = session.snapshot().outgoing[0].id.clone();

    // Before any game asks, the Player 2 code has no seat, and binds nothing.
    let r = guest.raw("GET", "/api/state", Vec::new()).await;
    assert_eq!((r.status, code(&r.body).as_str()), (403, "no-race"));
    assert!(session.snapshot().phone.is_none());

    // A two-player race: the guest is the very first phone to connect. It joins as Player 2, hears
    // only that there's a race (sealed with its own keys), and the session is still nobody's.
    session.pad_open(2);
    let (status, state) = guest.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        state.unwrap(),
        StateReply {
            offers: vec![],
            text: None,
            game: true,
            clip: None,
            guest: true
        }
    );
    assert!(session.snapshot().phone.is_none(), "a guest never becomes the owner");
    let (status, _) = guest.call::<PadReply>("POST", "/api/pad", input(-60, false)).await;
    assert_eq!(status, 200);
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert_eq!((last.seat, last.connected), (SECOND, true));

    // Nothing else, ever: text, the clipboard, uploads (begin, chunk, finish, cancel), downloads.
    let r = guest
        .call::<Value>("POST", "/api/text", Some(json!({ "text": "from the guest" })))
        .await;
    assert_eq!(r.0, 403);
    assert_eq!(
        guest
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": 1 })))
            .await
            .0,
        403
    );
    let file_id = "fileAAAAAAAAAAAAAAAAAA";
    let begin = json!({ "name": "a.jpg", "size": 10, "chunkSize": MIN_CHUNK });
    assert_eq!(
        guest
            .call::<UploadReply>("POST", &format!("/api/up/{file_id}"), Some(begin))
            .await
            .0,
        403
    );
    assert_eq!(guest.put_chunk(file_id, 0, b"0123456789").await, 403);
    assert_eq!(
        guest
            .raw("POST", &format!("/api/finish/{file_id}"), Vec::new())
            .await
            .status,
        403
    );
    assert_eq!(
        guest
            .raw("DELETE", &format!("/api/up/{file_id}"), Vec::new())
            .await
            .status,
        403
    );
    assert_eq!(
        guest
            .raw("GET", &format!("/api/down/{offer_id}/0"), Vec::new())
            .await
            .status,
        403
    );
    let snap = session.snapshot();
    assert!(snap.texts.is_empty() && snap.incoming.is_empty());
    assert_eq!(snap.outgoing[0].downloads, 0);
    assert!(sink.texts.lock().unwrap().is_empty());
    assert!(snap.phone.is_none(), "still nobody's after all that");

    // The owner scans Tugboat's own code: it binds exactly as before, and gets what's for it.
    let mut owner = Phone::new(addr, &secret, "ownerBBBBBBBBBBBBBBBBB");
    let (status, state) = owner.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(status, 200);
    let state = state.unwrap();
    assert!(!state.guest && state.game);
    assert_eq!(state.text.unwrap().text, "only for the owner");
    assert_eq!(state.offers.len(), 1);
    assert_eq!(session.snapshot().phone.as_deref(), Some("iPhone"));
    let (_, reply) = owner.call::<PadReply>("POST", "/api/pad", input(20, false)).await;
    assert!(reply.is_some());
    assert_eq!(sink.pads.lock().unwrap().last().unwrap().seat, FIRST);
    // The guest is still Player 2 alongside, and still only that.
    let (_, state) = guest.call::<StateReply>("GET", "/api/state", None).await;
    assert!(state.unwrap().guest);

    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// The phone's controller keeps reusing one kept-alive connection, even across a refusal (a reply
/// that left the body unread would close it and cost the phone a new connection).
#[tokio::test]
async fn controller_inputs_reuse_one_connection() {
    let root = temp_dir();
    let secret = crypto::new_secret();
    let session = Arc::new(Session::new(
        &secret,
        root.join("Tugboat"),
        root.join("incoming"),
        Arc::new(TestSink::default()),
    ));
    let (addr, stop, server) = start_server(session.clone(), server::Limits::default()).await;
    let keys = Keys::derive(&secret);
    let client = "phoneAAAAAAAAAAAAAAAAA";
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let conn = tokio::spawn(conn);
    let mut seq = 1_790_000_000_000u64 * 1024;
    let mut statuses = Vec::new();
    for i in 0..6 {
        // The game opens the controller after the first (refused) input.
        if i == 1 {
            session.pad_open(1);
        }
        seq += 1;
        let body = keys.seal(
            &ad::request(client, seq),
            format!(r#"{{"steer":{},"boost":false,"age":1,"rtt":2}}"#, i * 10).as_bytes(),
        );
        let req = Request::builder()
            .method("POST")
            .uri("/api/pad")
            .header("host", addr.to_string())
            .header("authorization", auth::header(&keys, client, seq, "POST", "/api/pad"))
            .body(Full::new(Bytes::from(body)))
            .unwrap();
        sender.ready().await.expect("the connection is still open");
        let res = sender.send_request(req).await.unwrap();
        statuses.push(res.status().as_u16());
        res.into_body().collect().await.unwrap();
    }
    assert_eq!(statuses, vec![409, 200, 200, 200, 200, 200]);
    assert!(!conn.is_finished(), "one connection carried every input");
    assert!(session.pad_latency().unwrap().starts_with("n=5,"));
    drop(sender);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

/// Not a test: the controller's round trip on localhost, input by input over one kept-alive
/// connection, as the phone sends them (signed, sealed, reply opened). Prints p50/p90/p99 so a
/// change to the channel can be compared before and after.
///   cargo test --manifest-path src-tauri/Cargo.toml controller_latency_bench -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn controller_latency_bench() {
    let root = temp_dir();
    let secret = crypto::new_secret();
    let session = Arc::new(Session::new(
        &secret,
        root.join("Tugboat"),
        root.join("incoming"),
        Arc::new(TestSink::default()),
    ));
    let (addr, stop, server) = start_server(session.clone(), server::Limits::default()).await;
    // BENCH_PLAYERS=2: a two-player game, with a second phone steering at the same pace alongside.
    let duo = std::env::var("BENCH_PLAYERS").is_ok_and(|v| v == "2");
    session.pad_open(if duo { 2 } else { 1 });
    let keys = Keys::derive(&secret);
    let client = "phoneAAAAAAAAAAAAAAAAA";
    let mut first = Phone::new(addr, &secret, client);
    assert_eq!(first.raw("GET", "/api/state", Vec::new()).await.status, 200);
    let guest = duo.then(|| {
        let keys = Keys::derive(&crypto::guest_secret(&secret));
        tokio::spawn(async move {
            let client = "secondBBBBBBBBBBBBBBBB";
            let stream = TcpStream::connect(addr).await.unwrap();
            let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
                .await
                .unwrap();
            tokio::spawn(conn);
            let mut seq = 1_700_000_000_000u64 * 1024;
            loop {
                seq += 1;
                let body = keys.seal(&ad::request(client, seq), br#"{"steer":30,"boost":false}"#);
                let req = Request::builder()
                    .method("POST")
                    .uri("/api/pad")
                    .header("host", addr.to_string())
                    .header("authorization", auth::header(&keys, client, seq, "POST", "/api/pad"))
                    .body(Full::new(Bytes::from(body)))
                    .unwrap();
                if sender.ready().await.is_err() {
                    return;
                }
                let Ok(res) = sender.send_request(req).await else {
                    return;
                };
                assert_eq!(res.status().as_u16(), 200);
                let _ = res.into_body().collect().await;
                tokio::time::sleep(std::time::Duration::from_millis(12)).await;
            }
        })
    });
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let mut seq = first.seq;
    let mut times = Vec::new();
    for i in 0..3000u32 {
        seq += 1;
        let started = std::time::Instant::now();
        let steer = (i % 200) as i64 - 100;
        let body = keys.seal(
            &ad::request(client, seq),
            format!(r#"{{"steer":{steer},"boost":false}}"#).as_bytes(),
        );
        let req = Request::builder()
            .method("POST")
            .uri("/api/pad")
            .header("host", addr.to_string())
            .header("authorization", auth::header(&keys, client, seq, "POST", "/api/pad"))
            .body(Full::new(Bytes::from(body)))
            .unwrap();
        sender.ready().await.unwrap();
        let res = sender.send_request(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        let reply = res.into_body().collect().await.unwrap().to_bytes();
        keys.open(&ad::response(client, seq), &reply).expect("reply opens");
        // Below the rate limit, like a phone at ~60 inputs a second would be (but faster, so the
        // bench doesn't take minutes): the bucket refills between inputs.
        if i >= 500 {
            times.push(started.elapsed());
        }
        tokio::time::sleep(std::time::Duration::from_millis(12)).await;
    }
    times.sort();
    let at = |p: f64| times[((times.len() as f64 * p) as usize).min(times.len() - 1)];
    if let Some(g) = guest {
        g.abort();
    }
    println!(
        "controller round trip on localhost ({} player(s)): n={}, p50 {:?}, p90 {:?}, p99 {:?}, max {:?}",
        if duo { 2 } else { 1 },
        times.len(),
        at(0.5),
        at(0.9),
        at(0.99),
        times[times.len() - 1]
    );
    drop(sender);
    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).ok();
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
    session.pad_open(1);
    session.pad_admit(&ok).unwrap();
    let body = keys.seal(&ad::request(client, seq), br#"{"steer":-100,"boost":false}"#);
    session.pad_input(&ok, &body).unwrap();
    assert!(sink.pads.lock().unwrap().last().unwrap().connected);
    session.pad_close();
    let last = *sink.pads.lock().unwrap().last().unwrap();
    assert!(!last.connected && last.steer == 0.0);
    std::fs::remove_dir_all(&root).ok();
}

/// The shared clipboard, over a real socket: the PC's clipboard reaches only the bound phone, is
/// "delivered" only once the phone says so (and, for a picture, really fetched it), and the PC lets
/// go of it then.
#[tokio::test]
async fn shared_clipboard_reaches_the_phone_and_is_let_go() {
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
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(state.unwrap().clip.is_none());

    // --- Text: waiting until the phone says it has it ---
    let id = session.send_clip(Prepared::Text("from the PC's clipboard".into()));
    let st = session.snapshot().clip.unwrap();
    assert_eq!((st.id, st.kind, st.state), (id, ClipKind::Text, ClipState::Waiting));
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(
        state.unwrap().clip,
        Some(PageClip {
            id,
            text: Some("from the PC's clipboard".into()),
            image: None
        })
    );
    assert!(sink.clips.lock().unwrap().is_empty(), "handed over isn't delivered");
    assert_eq!(session.snapshot().clip.unwrap().state, ClipState::Waiting);

    // Another device can't acknowledge it; nor can a made-up id, or a body with anything else in it.
    let mut second = Phone::new(addr, &secret, "secondBBBBBBBBBBBBBBBB");
    assert_eq!(
        second
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id })))
            .await
            .0,
        403
    );
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id + 50 })))
            .await
            .0,
        404
    );
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id, "text": "x" })))
            .await
            .0,
        400
    );
    assert_eq!(phone.raw("POST", "/api/clip", Vec::new()).await.status, 400);
    assert!(sink.clips.lock().unwrap().is_empty());

    // The phone has it: delivered, and the PC no longer hands it out.
    let (status, _) = phone
        .call::<Value>("POST", "/api/clip", Some(json!({ "id": id })))
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        *sink.clips.lock().unwrap(),
        vec![ClipEvent {
            kind: ClipKind::Text,
            state: ClipState::Delivered
        }]
    );
    assert_eq!(session.snapshot().clip.unwrap().state, ClipState::Delivered);
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert!(state.unwrap().clip.is_none());
    // A retry (the reply was lost) is fine and says nothing twice.
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id })))
            .await
            .0,
        200
    );
    assert_eq!(sink.clips.lock().unwrap().len(), 1);

    // --- A newer press replaces one still waiting ---
    let old = session.send_clip(Prepared::Text("first".into()));
    let newer = session.send_clip(Prepared::Text("second".into()));
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    assert_eq!(state.unwrap().clip.unwrap().text.as_deref(), Some("second"));
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": old })))
            .await
            .0,
        404
    );
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": newer })))
            .await
            .0,
        200
    );

    // --- A picture: fetched sealed, like a file on offer, but not listed with the files ---
    let picture = pattern(OFFER_CHUNK as usize + 4321, 9);
    let id = session.send_clip(Prepared::Image(picture.clone()));
    assert!(session.snapshot().outgoing.is_empty(), "not in the panel's file list");
    let (_, state) = phone.call::<StateReply>("GET", "/api/state", None).await;
    let state = state.unwrap();
    assert!(state.offers.is_empty(), "not in the page's file list");
    let clip = state.clip.unwrap();
    assert_eq!((clip.id, clip.text), (id, None));
    let image = clip.image.unwrap();
    assert_eq!(
        (image.name.as_str(), image.mime.as_str(), image.size, image.chunks),
        ("Clipboard picture.png", "image/png", picture.len() as u64, 2)
    );
    // The phone's word isn't enough until it really fetched the picture.
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id })))
            .await
            .0,
        409
    );
    let mut got = Vec::new();
    for i in 0..image.chunks {
        let r = phone
            .raw("GET", &format!("/api/down/{}/{i}", image.id), Vec::new())
            .await;
        assert_eq!(r.status, 200);
        got.extend(phone.keys.open(&ad::down(&image.id, i), &r.body).expect("chunk opens"));
    }
    assert_eq!(got, picture);
    assert_eq!(
        phone
            .call::<Value>("POST", "/api/clip", Some(json!({ "id": id })))
            .await
            .0,
        200
    );
    assert_eq!(
        sink.clips.lock().unwrap().last().copied(),
        Some(ClipEvent {
            kind: ClipKind::Image,
            state: ClipState::Delivered
        })
    );
    // Delivered: the PC let go of the picture.
    assert_eq!(
        phone
            .raw("GET", &format!("/api/down/{}/0", image.id), Vec::new())
            .await
            .status,
        404
    );

    // Nothing the clipboard did reached the phone's paste box (the PC clipboard) or the files.
    assert!(sink.texts.lock().unwrap().is_empty());
    let snap = session.snapshot();
    assert!(snap.texts.is_empty() && snap.incoming.is_empty() && snap.outgoing.is_empty());

    let _ = stop.send(());
    server.await.unwrap();
    std::fs::remove_dir_all(&root).unwrap();
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
/// TUGBOAT_CLIP=1 sends a made-up clipboard text (and prints when the page acknowledges it).
/// TUGBOAT_HOST=<this PC's LAN address> serves there instead, so the page runs the way a phone
/// sees it (not a secure context: no `navigator.clipboard`).
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
    if let Ok(game) = std::env::var("TUGBOAT_GAME") {
        // TUGBOAT_GAME=2: a two-player game (a second browser can join as Player 2).
        session.pad_open(if game == "2" { 2 } else { 1 });
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
                if events.len() != shown {
                    if let Some(l) = watched.pad_latency() {
                        println!("controller latency (touch to PC): {l}");
                    }
                }
                shown = events.len();
            }
        });
    }
    if std::env::var("TUGBOAT_CLIP").is_ok() {
        session.send_clip(Prepared::Text(
            "Your clipboard from the PC: 221B Example Street, a link https://example.com/x".into(),
        ));
        let watched = session.clone();
        let clips = sink.clone();
        tokio::spawn(async move {
            let mut shown = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                watched.clip_tick();
                let events = clips.clips.lock().unwrap().clone();
                for e in &events[shown..] {
                    println!("clipboard: {e:?}");
                }
                shown = events.len();
            }
        });
    }
    let host = std::env::var("TUGBOAT_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let listener = TcpListener::bind((host.as_str(), 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    println!("Tugboat page: http://{addr}/#{}", crypto::b64(&secret));
    if session.wants_guest() {
        println!(
            "Player 2 code: http://{addr}/#{}{}",
            crypto::GUEST_PREFIX,
            crypto::b64(&crypto::guest_secret(&secret))
        );
    }
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
