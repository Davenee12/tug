//! Message Notification Server (MNS): the OBEX server the phone connects *to* once
//! `SetNotificationRegistration(on)` has been sent on the MAS session. iOS opens an RFCOMM
//! link to the service class tug advertises (MNS, 0x1133), runs OBEX CONNECT with the MNS
//! target UUID, and PUTs `x-bt/MAP-event-report` objects (`map::mns_event`) as messages
//! arrive or sends complete. tug answers each with OK and turns the report into an action.
//!
//! Layering mirrors the client side: the SDP record bytes, the per-connection OBEX state
//! machine, the event→action mapping and the "which outgoing message did this confirm"
//! pick are all pure and unit-tested here; the WinRT `RfcommServiceProvider` /
//! `StreamSocketListener` plumbing is thin, `#[cfg(windows)]`, and can only be proven on
//! hardware.

use std::time::Duration;

use super::mns_event::{EventType, MnsEvent};
use super::obex::{self, Request};

/// Inbox poll cadences. The ~8 s poll is the backstop; live texts let it relax to 30 s, but
/// only while event reports are actually arriving (so a quiet or failed MNS still polls at 8 s).
pub const POLL_WATCHING: Duration = Duration::from_secs(2);
pub const POLL_CONNECTED: Duration = Duration::from_secs(8);
pub const POLL_CONNECTED_LIVE: Duration = Duration::from_secs(30);
pub const POLL_DISCONNECTED: Duration = Duration::from_secs(30);

/// How long until the next inbox poll. `watching` (the user on the switches screen) is fastest;
/// a connected session polls at 8 s, or 30 s when live texts are fresh; no session retries at 30 s.
pub fn poll_after(watching: bool, connected: bool, live_fresh: bool) -> Duration {
    if watching {
        POLL_WATCHING
    } else if connected {
        if live_fresh {
            POLL_CONNECTED_LIVE
        } else {
            POLL_CONNECTED
        }
    } else {
        POLL_DISCONNECTED
    }
}

/// SDP attribute id for the human-readable service name (`ServiceName`).
pub const ATTR_SERVICE_NAME: u32 = 0x0100;
/// SDP attribute id for the `BluetoothProfileDescriptorList`.
pub const ATTR_PROFILE_DESCRIPTOR_LIST: u32 = 0x0009;
/// MAP profile UUID carried in the profile descriptor list (not the MNS *service* class).
pub const MAP_PROFILE_UUID: u16 = 0x1134;
/// MAP profile version we claim: 1.1 (`0x0101`). iOS has shipped MAP 1.x for years; 1.2
/// (`0x0102`) is the other safe choice if a future iOS ever ignores 1.1. Bump this constant,
/// not the encoder, to try it.
pub const MAP_PROFILE_VERSION: u16 = 0x0101;
/// The name shown in our SDP record. Short, so it fits a 1-byte length descriptor.
pub const SERVICE_NAME: &str = "tug MNS";

/// The ConnectionId tug hands the phone in the CONNECT response; it quotes this on every
/// later request. One MNS connection at a time, so a fixed value is enough.
pub const MNS_CONNECTION_ID: u32 = 1;

/// Encode the `ServiceName` SDP attribute value: an OBEX/SDP text-string data element
/// (`0x25` = text string with a 1-byte length) holding the UTF-8 name.
pub fn service_name_attribute(name: &str) -> Vec<u8> {
    let bytes = name.as_bytes();
    let len = bytes.len().min(u8::MAX as usize);
    let mut out = Vec::with_capacity(len + 2);
    out.push(0x25);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
    out
}

/// Encode the `BluetoothProfileDescriptorList` value: a sequence of one `(UUID16, version)`
/// pair — here `(MAP 0x1134, version)`. Data-element tags: `0x35` sequence (1-byte length),
/// `0x19` UUID (2 bytes), `0x09` unsigned int (2 bytes).
pub fn profile_descriptor_list_attribute() -> Vec<u8> {
    let [uhi, ulo] = MAP_PROFILE_UUID.to_be_bytes();
    let [vhi, vlo] = MAP_PROFILE_VERSION.to_be_bytes();
    // Inner pair: UUID16 + uint16 = 6 bytes.
    let inner = [0x19, uhi, ulo, 0x09, vhi, vlo];
    let mut list = vec![0x35, inner.len() as u8];
    list.extend_from_slice(&inner);
    // Outer list wraps the one pair-sequence.
    let mut out = vec![0x35, list.len() as u8];
    out.extend_from_slice(&list);
    out
}

/// What the worker should do about one received event. Keeping the decision pure means the
/// mapping (and its awkward cases) is testable without a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventAction {
    /// Pull the inbox now: a message landed (or an event we can't place, so refresh to be safe).
    Refresh,
    /// A send completed; confirm the matching outgoing message. Handle is the report's, which
    /// may not be the one PushMessage returned (see [`choose_outgoing`]).
    SendConfirmed { handle: Option<String> },
    /// A send failed; mark the matching outgoing message failed.
    SendFailed { handle: Option<String> },
    /// Nothing to do (memory events, read-status changes we poll anyway, deletions we don't mirror).
    Ignore,
}

/// Map an event report to an action. Conservative: a `NewMessage` in the inbox (or an event
/// type we don't recognise) triggers a refresh so nothing is missed, while events about other
/// folders or ones the poll already covers are ignored.
pub fn event_action(event: &MnsEvent) -> EventAction {
    match event.kind {
        EventType::NewMessage => match event.folder.as_deref() {
            // iOS reports inbound texts as NewMessage in the inbox; some omit the folder.
            Some("inbox") | None => EventAction::Refresh,
            // A NewMessage in sent/outbox is the echo of our own send; SendingSuccess covers it.
            Some(_) => EventAction::Ignore,
        },
        EventType::SendingSuccess => EventAction::SendConfirmed {
            handle: event.handle.clone(),
        },
        EventType::SendingFailure => EventAction::SendFailed {
            handle: event.handle.clone(),
        },
        // Unknown (MAP 1.3+ conversation/presence events, or a sloppy phone): refresh rather
        // than silently drop a possible new message.
        EventType::Unknown => EventAction::Refresh,
        // Delivery is carrier-side and tug has no "delivered" state; memory, shift, deletion and
        // read-status are either irrelevant or already covered by the poll.
        EventType::DeliverySuccess
        | EventType::DeliveryFailure
        | EventType::MemoryFull
        | EventType::MemoryAvailable
        | EventType::MessageDeleted
        | EventType::MessageShift
        | EventType::ReadStatusChanged => EventAction::Ignore,
    }
}

/// An unconfirmed outgoing message, as the picker needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub id: i64,
    /// The handle iOS returned from PushMessage, if any.
    pub handle: Option<String>,
    pub received_at: i64,
}

/// Which still-unconfirmed outgoing message a Sending{Success,Failure} report is about.
///
/// Handle first: if the report's handle equals one a PushMessage stored, that's the one.
/// The roadmap's risk is that iOS's SendingSuccess handle differs from the PushMessage
/// handle — and the report carries no recipient — so when the handle doesn't match we fall
/// back to the most recent unconfirmed send. Sends run one at a time through the MAP worker,
/// so the newest pending/accepted message is the one that just completed.
pub fn choose_outgoing(report_handle: Option<&str>, candidates: &[Outgoing]) -> Option<i64> {
    if let Some(h) = report_handle.filter(|h| !h.is_empty()) {
        if let Some(m) = candidates.iter().find(|c| c.handle.as_deref() == Some(h)) {
            return Some(m.id);
        }
    }
    candidates
        .iter()
        .max_by(|a, b| a.received_at.cmp(&b.received_at).then(a.id.cmp(&b.id)))
        .map(|m| m.id)
}

/// Per-connection OBEX server state: accumulates a (possibly multi-packet) PUT body and
/// answers each request. Pure so the request/response dance is tested without a socket.
#[derive(Debug, Default)]
pub struct MnsConn {
    body: Vec<u8>,
    connected: bool,
}

/// What [`MnsConn::handle`] decided: the bytes to write back, an event to act on, and whether
/// the link is finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAction {
    pub response: Vec<u8>,
    pub event: Option<MnsEvent>,
    pub close: bool,
}

impl MnsConn {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the phone has completed OBEX CONNECT.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Process one request the phone sent and decide the response. A completed event-report
    /// PUT yields the parsed event (or, if it won't parse, `None` with an OK so the phone
    /// doesn't retry forever — the poll is the backstop).
    pub fn handle(&mut self, req: &Request) -> ServerAction {
        // CONNECT: accept the directed MNS session (Who echoes the target the phone asked for).
        if req.connect.is_some() {
            self.connected = true;
            self.body.clear();
            let who = connect_target(req).unwrap_or(super::mns_event::MNS_TARGET);
            return ServerAction {
                response: obex::connect_success(MNS_CONNECTION_ID, &who),
                event: None,
                close: false,
            };
        }
        // DISCONNECT: acknowledge and close.
        if req.opcode == obex::OP_DISCONNECT & !obex::FINAL_BIT {
            return ServerAction {
                response: obex::response(obex::RSP_SUCCESS, &[]),
                event: None,
                close: true,
            };
        }
        // ABORT: drop the half-built object, keep the link.
        if req.opcode == obex::OP_ABORT {
            self.body.clear();
            return ServerAction {
                response: obex::response(obex::RSP_SUCCESS, &[]),
                event: None,
                close: false,
            };
        }
        // PUT (final or not): the phone pushes the event report as the body.
        if req.opcode == obex::OP_PUT {
            self.body.extend_from_slice(&req.body());
            if !req.is_final {
                return ServerAction {
                    response: obex::response(obex::RSP_CONTINUE, &[]),
                    event: None,
                    close: false,
                };
            }
            let body = std::mem::take(&mut self.body);
            let event = match super::mns_event::parse_event_report(&body) {
                Ok(e) => Some(e),
                Err(e) => {
                    log::debug!("unparseable MAP event report ({e}); leaving it to the poll");
                    None
                }
            };
            return ServerAction {
                response: obex::response(obex::RSP_SUCCESS, &[]),
                event,
                close: false,
            };
        }
        // Anything else (a GET, say): MNS only receives CONNECT/PUT/DISCONNECT.
        ServerAction {
            response: obex::response(obex::RSP_BAD_REQUEST, &[]),
            event: None,
            close: false,
        }
    }
}

/// The 16-byte Target a CONNECT asked for, if it gave a full one.
fn connect_target(req: &Request) -> Option<[u8; 16]> {
    req.target()?.try_into().ok()
}

#[cfg(windows)]
pub use server::{start, MnsServer, ServerMessage};

/// The WinRT plumbing: advertise the service, accept the phone's connection, and run the pure
/// `MnsConn` over the socket. Thin on purpose; everything decidable lives above.
#[cfg(windows)]
mod server {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::mpsc::UnboundedSender;
    use windows::core::GUID;
    use windows::Devices::Bluetooth::Rfcomm::{RfcommServiceId, RfcommServiceProvider};
    use windows::Foundation::TypedEventHandler;
    use windows::Networking::Sockets::{
        SocketProtectionLevel, StreamSocket, StreamSocketListener, StreamSocketListenerConnectionReceivedEventArgs,
    };
    use windows::Storage::Streams::{DataReader, DataWriter, IBuffer, InputStreamOptions};

    use super::{MnsConn, SERVICE_NAME};
    use crate::map::mns_event::{MnsEvent, MNS_UUID};
    use crate::map::obex;
    use crate::map::session::MapError;

    /// Longest any single WinRT MNS operation may block; a stuck accept, read or write must
    /// give up rather than wedge the connection-service task. Matches the GATT bound.
    const OP_TIMEOUT: Duration = Duration::from_secs(10);

    /// Bound a WinRT await, mapping a stall to `Timeout` like the rest of the MAP code.
    async fn bounded<T>(op: impl std::future::IntoFuture<Output = windows::core::Result<T>>) -> Result<T, MapError> {
        Ok(tokio::time::timeout(OP_TIMEOUT, op)
            .await
            .map_err(|_| MapError::Timeout)??)
    }

    /// What the server tells the worker.
    #[derive(Debug, Clone)]
    pub enum ServerMessage {
        /// The phone opened the MNS link (completed OBEX CONNECT).
        Connected,
        /// The phone pushed an event report.
        Event(MnsEvent),
    }

    /// A running MNS advertisement + listener. Dropping it stops advertising and unhooks the
    /// connection handler, so a torn-down session can't leave a stale SDP record or handler.
    pub struct MnsServer {
        provider: RfcommServiceProvider,
        listener: StreamSocketListener,
        token: i64,
        /// Flipped on drop so an in-flight service task stops touching the socket.
        stopped: Arc<AtomicBool>,
    }

    impl Drop for MnsServer {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
            if let Err(e) = self.listener.RemoveConnectionReceived(self.token) {
                log::debug!("removing MNS connection handler failed: {e}");
            }
            if let Err(e) = self.provider.StopAdvertising() {
                log::debug!("stopping MNS advertising failed: {e}");
            }
        }
    }

    /// Advertise the MNS service and start listening. The caller must already hold a MAP (MAS)
    /// session; it sends `SetNotificationRegistration(on)` *after* this returns so the phone has
    /// a record to find. Events arrive on `tx`.
    pub async fn start(tx: UnboundedSender<ServerMessage>) -> Result<MnsServer, MapError> {
        let service_id = RfcommServiceId::FromUuid(GUID::from_u128(MNS_UUID))?;
        let provider = bounded(RfcommServiceProvider::CreateAsync(&service_id)?).await?;
        let listener = StreamSocketListener::new()?;

        let stopped = Arc::new(AtomicBool::new(false));
        let handler_stopped = stopped.clone();
        let runtime = tokio::runtime::Handle::current();
        let token = listener.ConnectionReceived(&TypedEventHandler::<
            StreamSocketListener,
            StreamSocketListenerConnectionReceivedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let socket = args.Socket()?;
            let tx = tx.clone();
            let stopped = handler_stopped.clone();
            // The event fires on a WinRT pool thread; hand the socket to the worker's runtime,
            // which owns all the other MAP I/O.
            runtime.spawn(async move {
                if let Err(e) = serve_connection(socket, tx, stopped).await {
                    log::info!("MNS connection ended: {e}");
                }
            });
            Ok(())
        }))?;

        // Bind the listener to the provider's service name, then publish the SDP extras and
        // advertise. Protection level matches the MAS link (encryption, null auth allowed).
        bounded(listener.BindServiceNameWithProtectionLevelAsync(
            &provider.ServiceId()?.AsString()?,
            SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
        )?)
        .await?;

        let attrs = provider.SdpRawAttributes()?;
        attrs.Insert(
            super::ATTR_SERVICE_NAME,
            &to_buffer(&super::service_name_attribute(SERVICE_NAME))?,
        )?;
        attrs.Insert(
            super::ATTR_PROFILE_DESCRIPTOR_LIST,
            &to_buffer(&super::profile_descriptor_list_attribute())?,
        )?;

        // Not radio-discoverable: the phone finds this over the existing connection's SDP, not
        // an inquiry scan.
        provider.StartAdvertising(&listener)?;
        Ok(MnsServer {
            provider,
            listener,
            token,
            stopped,
        })
    }

    fn to_buffer(bytes: &[u8]) -> windows::core::Result<IBuffer> {
        let w = DataWriter::new()?;
        w.WriteBytes(bytes)?;
        w.DetachBuffer()
    }

    /// Service one accepted MNS connection: run the OBEX server state machine until the phone
    /// disconnects or the link closes.
    async fn serve_connection(
        socket: StreamSocket,
        tx: UnboundedSender<ServerMessage>,
        stopped: Arc<AtomicBool>,
    ) -> Result<(), MapError> {
        let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
        let mut conn = MnsConn::new();
        loop {
            if stopped.load(Ordering::SeqCst) {
                let _ = socket.Close();
                return Ok(());
            }
            let packet = match read_packet(&reader).await {
                Ok(p) => p,
                // A clean close is how the phone ends the link; report it as done.
                Err(MapError::Closed) => return Ok(()),
                Err(e) => return Err(e),
            };
            let req = obex::parse_request(&packet)?;
            let was_connected = conn.is_connected();
            let action = conn.handle(&req);
            write_packet(&writer, &action.response).await?;
            if !was_connected && conn.is_connected() {
                let _ = tx.send(ServerMessage::Connected);
            }
            if let Some(event) = action.event {
                let _ = tx.send(ServerMessage::Event(event));
            }
            if action.close {
                let _ = socket.Close();
                return Ok(());
            }
        }
    }

    /// Read exactly one OBEX packet, using its 3-byte length prefix (mirrors the client link).
    async fn read_packet(reader: &DataReader) -> Result<Vec<u8>, MapError> {
        let mut buf = Vec::new();
        loop {
            let need = match obex::packet_len(&buf) {
                Some(len) if buf.len() >= len => return Ok(buf),
                Some(len) => len - buf.len(),
                None => 3 - buf.len(),
            };
            let got = bounded(reader.LoadAsync(need as u32)?).await?;
            if got == 0 {
                return Err(MapError::Closed);
            }
            let mut chunk = vec![0; got as usize];
            reader.ReadBytes(&mut chunk)?;
            buf.extend_from_slice(&chunk);
        }
    }

    async fn write_packet(writer: &DataWriter, packet: &[u8]) -> Result<(), MapError> {
        writer.WriteBytes(packet)?;
        bounded(writer.StoreAsync()?).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::mns_event::{MnsEvent, MNS_TARGET};
    use crate::map::obex::{self, Header};

    fn ev(kind: EventType, handle: Option<&str>, folder: Option<&str>) -> MnsEvent {
        MnsEvent {
            kind,
            handle: handle.map(str::to_string),
            folder: folder.map(str::to_string),
            old_folder: None,
            msg_type: None,
            datetime: None,
        }
    }

    #[test]
    fn service_name_is_a_text_data_element() {
        assert_eq!(
            service_name_attribute("tug MNS"),
            [0x25, 7, b't', b'u', b'g', b' ', b'M', b'N', b'S']
        );
        // Empty and over-long names stay well formed.
        assert_eq!(service_name_attribute(""), [0x25, 0]);
        let long = "x".repeat(300);
        let a = service_name_attribute(&long);
        assert_eq!(a[0], 0x25);
        assert_eq!(a[1], 255);
        assert_eq!(a.len(), 257);
    }

    #[test]
    fn poll_relaxes_only_when_live_is_fresh() {
        // Watching beats everything.
        assert_eq!(poll_after(true, true, true), POLL_WATCHING);
        assert_eq!(poll_after(true, false, false), POLL_WATCHING);
        // Connected: 8 s normally, 30 s only while events are fresh.
        assert_eq!(poll_after(false, true, false), POLL_CONNECTED);
        assert_eq!(poll_after(false, true, true), POLL_CONNECTED_LIVE);
        // No session: slow retry.
        assert_eq!(poll_after(false, false, false), POLL_DISCONNECTED);
        // "Live fresh" without a session is meaningless and must not relax the retry.
        assert_eq!(poll_after(false, false, true), POLL_DISCONNECTED);
    }

    #[test]
    fn profile_list_is_map_1_1() {
        assert_eq!(
            profile_descriptor_list_attribute(),
            [0x35, 0x08, 0x35, 0x06, 0x19, 0x11, 0x34, 0x09, 0x01, 0x01]
        );
    }

    #[test]
    fn new_inbox_message_refreshes() {
        assert_eq!(
            event_action(&ev(EventType::NewMessage, Some("1"), Some("inbox"))),
            EventAction::Refresh
        );
        assert_eq!(
            event_action(&ev(EventType::NewMessage, Some("1"), None)),
            EventAction::Refresh
        );
    }

    #[test]
    fn new_sent_message_is_ignored() {
        assert_eq!(
            event_action(&ev(EventType::NewMessage, Some("1"), Some("sent"))),
            EventAction::Ignore
        );
    }

    #[test]
    fn sending_outcomes_carry_the_handle() {
        assert_eq!(
            event_action(&ev(EventType::SendingSuccess, Some("A1"), Some("sent"))),
            EventAction::SendConfirmed {
                handle: Some("A1".into())
            }
        );
        assert_eq!(
            event_action(&ev(EventType::SendingFailure, Some("A1"), Some("outbox"))),
            EventAction::SendFailed {
                handle: Some("A1".into())
            }
        );
    }

    #[test]
    fn unknown_events_refresh_known_quiet_ones_do_not() {
        assert_eq!(
            event_action(&ev(EventType::Unknown, Some("9"), Some("inbox"))),
            EventAction::Refresh
        );
        for kind in [
            EventType::DeliverySuccess,
            EventType::DeliveryFailure,
            EventType::MemoryFull,
            EventType::MemoryAvailable,
            EventType::MessageDeleted,
            EventType::MessageShift,
            EventType::ReadStatusChanged,
        ] {
            assert_eq!(
                event_action(&ev(kind, Some("1"), Some("inbox"))),
                EventAction::Ignore,
                "{kind:?}"
            );
        }
    }

    fn out(id: i64, handle: Option<&str>, received_at: i64) -> Outgoing {
        Outgoing {
            id,
            handle: handle.map(str::to_string),
            received_at,
        }
    }

    #[test]
    fn matches_a_send_by_handle_first() {
        let candidates = [out(1, Some("AA"), 100), out(2, Some("BB"), 200)];
        assert_eq!(choose_outgoing(Some("AA"), &candidates), Some(1));
        assert_eq!(choose_outgoing(Some("BB"), &candidates), Some(2));
    }

    #[test]
    fn falls_back_to_the_newest_unconfirmed_send() {
        // iOS's report handle doesn't match any stored one: take the most recent pending send.
        let candidates = [out(1, Some("AA"), 100), out(2, Some("BB"), 300), out(3, None, 200)];
        assert_eq!(choose_outgoing(Some("ZZ"), &candidates), Some(2));
        assert_eq!(choose_outgoing(None, &candidates), Some(2));
        // Ties on time break by id, so the pick is deterministic.
        let tied = [out(5, None, 300), out(6, None, 300)];
        assert_eq!(choose_outgoing(None, &tied), Some(6));
    }

    #[test]
    fn empty_report_handle_falls_back_to_newest() {
        let candidates = [out(1, Some("AA"), 50), out(2, Some("BB"), 100)];
        // An empty report handle never matches; fall back to the newest send.
        assert_eq!(choose_outgoing(Some(""), &candidates), Some(2));
    }

    #[test]
    fn no_candidates_is_none() {
        assert_eq!(choose_outgoing(Some("AA"), &[]), None);
    }

    fn report(event: &str) -> Vec<u8> {
        format!("<MAP-event-report version=\"1.0\">{event}</MAP-event-report>").into_bytes()
    }

    #[test]
    fn connect_is_accepted_with_who() {
        let mut conn = MnsConn::new();
        let req = obex::parse_request(&obex::connect(&MNS_TARGET)).unwrap();
        let action = conn.handle(&req);
        assert!(conn.is_connected());
        assert!(action.event.is_none());
        assert!(!action.close);
        let resp = obex::parse_response(&action.response, true).unwrap();
        assert!(resp.is_success());
        assert_eq!(resp.connection_id(), Some(MNS_CONNECTION_ID));
    }

    #[test]
    fn a_single_put_yields_the_event_and_an_ok() {
        let mut conn = MnsConn::new();
        conn.handle(&obex::parse_request(&obex::connect(&MNS_TARGET)).unwrap());
        let body = report(r#"<event type="NewMessage" handle="42" folder="TELECOM/MSG/INBOX"/>"#);
        let put = obex::request(
            obex::OP_PUT_FINAL,
            &[],
            &[
                Header::connection_id(MNS_CONNECTION_ID),
                Header::type_("x-bt/MAP-event-report"),
                Header::Bytes(obex::HI_END_OF_BODY, body),
            ],
        );
        let action = conn.handle(&obex::parse_request(&put).unwrap());
        assert_eq!(
            obex::parse_response(&action.response, false).unwrap().code,
            obex::RSP_SUCCESS
        );
        let event = action.event.expect("event parsed");
        assert_eq!(event.kind, EventType::NewMessage);
        assert_eq!(event.handle.as_deref(), Some("42"));
    }

    #[test]
    fn a_multi_packet_put_is_reassembled() {
        let mut conn = MnsConn::new();
        conn.handle(&obex::parse_request(&obex::connect(&MNS_TARGET)).unwrap());
        let whole = report(r#"<event type="SendingSuccess" handle="7" folder="TELECOM/MSG/SENT"/>"#);
        let (first, second) = whole.split_at(whole.len() / 2);
        let p1 = obex::request(obex::OP_PUT, &[], &[Header::Bytes(obex::HI_BODY, first.to_vec())]);
        let a1 = conn.handle(&obex::parse_request(&p1).unwrap());
        assert_eq!(
            obex::parse_response(&a1.response, false).unwrap().code,
            obex::RSP_CONTINUE
        );
        assert!(a1.event.is_none());
        let p2 = obex::request(
            obex::OP_PUT_FINAL,
            &[],
            &[Header::Bytes(obex::HI_END_OF_BODY, second.to_vec())],
        );
        let a2 = conn.handle(&obex::parse_request(&p2).unwrap());
        assert_eq!(
            obex::parse_response(&a2.response, false).unwrap().code,
            obex::RSP_SUCCESS
        );
        assert_eq!(a2.event.unwrap().kind, EventType::SendingSuccess);
    }

    #[test]
    fn an_unparseable_report_still_gets_an_ok_without_an_event() {
        let mut conn = MnsConn::new();
        conn.handle(&obex::parse_request(&obex::connect(&MNS_TARGET)).unwrap());
        let put = obex::request(
            obex::OP_PUT_FINAL,
            &[],
            &[Header::Bytes(obex::HI_END_OF_BODY, b"not xml".to_vec())],
        );
        let action = conn.handle(&obex::parse_request(&put).unwrap());
        assert_eq!(
            obex::parse_response(&action.response, false).unwrap().code,
            obex::RSP_SUCCESS
        );
        assert!(action.event.is_none());
    }

    #[test]
    fn disconnect_closes_and_abort_clears() {
        let mut conn = MnsConn::new();
        conn.handle(&obex::parse_request(&obex::connect(&MNS_TARGET)).unwrap());
        // A half-sent object, then ABORT: the buffer is dropped, link stays open.
        let partial = obex::request(obex::OP_PUT, &[], &[Header::Bytes(obex::HI_BODY, b"<MAP".to_vec())]);
        conn.handle(&obex::parse_request(&partial).unwrap());
        let abort = conn.handle(&obex::parse_request(&obex::request(obex::OP_ABORT, &[], &[])).unwrap());
        assert!(!abort.close);
        assert_eq!(
            obex::parse_response(&abort.response, false).unwrap().code,
            obex::RSP_SUCCESS
        );
        // DISCONNECT closes.
        let disc = conn.handle(
            &obex::parse_request(&obex::request(
                obex::OP_DISCONNECT,
                &[],
                &[Header::connection_id(MNS_CONNECTION_ID)],
            ))
            .unwrap(),
        );
        assert!(disc.close);
        assert_eq!(
            obex::parse_response(&disc.response, false).unwrap().code,
            obex::RSP_SUCCESS
        );
    }
}
