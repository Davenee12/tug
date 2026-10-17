//! OBEX sessions with the iPhone over WinRT RFCOMM `StreamSocket`s (WinRT resolves
//! each service's RFCOMM channel from SDP): MAP for messages, PBAP for contacts and
//! call history.

use std::time::Duration;

use thiserror::Error;
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::{SocketProtectionLevel, StreamSocket};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

use super::bmessage::{self, BMessage};
use super::calls::{self, CallDirection, CallRecord};
use super::charset;
use super::contacts_watch::Fingerprint;
use super::listing::{self, ListedMessage};
use super::obex::{self, Header, ObexError, Response};
use super::vcard::{self, PhonebookEntry};

/// Message Access Server service class (SDP).
pub const MAS_UUID: u128 = 0x0000_1132_0000_1000_8000_0080_5F9B_34FB;
/// OBEX Target for MAS sessions.
pub const MAS_TARGET: [u8; 16] = [
    0xBB, 0x58, 0x2B, 0x40, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];
/// Phonebook Access Server Equipment service class (SDP).
pub const PSE_UUID: u128 = 0x0000_112F_0000_1000_8000_0080_5F9B_34FB;
/// OBEX Target for PBAP sessions.
pub const PBAP_TARGET: [u8; 16] = [
    0x79, 0x61, 0x35, 0xF0, 0xF0, 0xC5, 0x11, 0xD8, 0x09, 0x66, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

/// The phone serves one PBAP connection at a time (a second fails with "only one usage of each
/// socket address"), so whoever opens one holds this turn for as long as it's open: tug's contacts
/// and calls pulls, the contact-photo pass, and the Bluetooth inventory's phonebook check. Always
/// `try_lock`: a busy turn means "defer" (or, for the inventory, "skip"), never wait — the MAP
/// worker that runs the contacts and calls pulls also carries texts, which must not stall.
pub static PBAP_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The "attachment line in another language" shape is logged once per launch: enough to learn
/// a language exists, without a line per text.
static ATTACHMENT_SHAPE_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
/// Device lookup + RFCOMM connect + OBEX CONNECT, end to end.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

// MAP application parameter tags.
const AP_MAX_LIST_COUNT: u8 = 0x01;
const AP_LIST_START_OFFSET: u8 = 0x02;
const AP_ATTACHMENT: u8 = 0x0A;
const AP_NOTIFICATION_STATUS: u8 = 0x0E;
const AP_MAS_INSTANCE_ID: u8 = 0x0F;
const AP_CHARSET: u8 = 0x14;
const AP_STATUS_INDICATOR: u8 = 0x17;
const AP_STATUS_VALUE: u8 = 0x18;
const CHARSET_UTF8: u8 = 0x01;
const STATUS_READ: u8 = 0x00;

// PBAP application parameter tags.
const PB_MAX_LIST_COUNT: u8 = 0x04;
const PB_PROPERTY_SELECTOR: u8 = 0x06;
const PB_FORMAT: u8 = 0x07;
const PB_FORMAT_VCARD30: u8 = 0x01;
/// In the answer to a PullPhoneBook with MaxListCount 0 (PBAP 1.2 for the last three).
const PB_PHONEBOOK_SIZE: u8 = 0x08;
const PB_PRIMARY_COUNTER: u8 = 0x0A;
const PB_SECONDARY_COUNTER: u8 = 0x0B;
const PB_DATABASE_ID: u8 = 0x0D;
/// PbapSupportedFeatures, sent in the CONNECT so a PBAP 1.2 phone includes its counters.
const PB_SUPPORTED_FEATURES: u8 = 0x10;
/// What tug does as a PBAP client: download (bit 0), browsing (1), database identifier (2) and
/// folder version counters (3). Nothing that changes what a pull returns.
const PCE_FEATURES: u32 = 0b1111;
/// PropertySelector bits: VERSION, FN, N, TEL.
const PB_PROPERTIES: u64 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 7);
/// Contacts also ask for PHOTO (bit 3), so the iPhone inlines a face when it has one. iOS may or
/// may not include it even when asked — the parser and the UI both fall back to initials — so this
/// stays a hint, confirmed only on hardware.
const PB_CONTACT_PROPERTIES: u64 = PB_PROPERTIES | (1 << 3);
/// The base set plus X-IRMC-CALL-DATETIME (bit 28), which carries a call's direction and time.
/// Call history has no use for photos, so it keeps asking only for the essentials.
const PB_CALL_PROPERTIES: u64 = PB_PROPERTIES | (1 << 28);

#[derive(Debug, Error)]
pub enum MapError {
    #[error("no paired device offers Bluetooth message access")]
    NoDevice,
    #[error("the iPhone isn't offering this service over Bluetooth")]
    NoService,
    #[error(
        "the iPhone refused message access; turn on Show Notifications for this PC under Settings › Bluetooth › ⓘ"
    )]
    Consent,
    #[error("the iPhone refused contact access; turn on Sync Contacts for this PC under Settings › Bluetooth › ⓘ")]
    ContactsConsent,
    #[error("{op} failed (OBEX {code:#04x})")]
    Obex { op: &'static str, code: u8 },
    #[error("the iPhone didn't answer in time")]
    Timeout,
    #[error("the connection closed")]
    Closed,
    #[error("bad OBEX data: {0}")]
    Parse(#[from] ObexError),
    #[error("Windows Bluetooth error: {}", .0.message())]
    Win(#[from] windows::core::Error),
    #[error("storage error: {0}")]
    Store(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, MapError>;

/// A PushMessage that failed, and whether the phone may have taken it anyway: the final packet
/// went out but its answer never came (or the link died waiting), so the text may be on its way.
#[derive(Debug)]
pub struct PushError {
    pub error: MapError,
    pub maybe_taken: bool,
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

// Defined in the pure `pick` module (with the selection logic that is unit-tested there).
pub use super::pick::MapDevice;

fn service_id(uuid: u128) -> windows::core::Result<RfcommServiceId> {
    RfcommServiceId::FromUuid(GUID::from_u128(uuid))
}

/// Paired Classic devices that advertise a Message Access Server.
pub async fn find_devices() -> Result<Vec<MapDevice>> {
    tokio::time::timeout(CONNECT_TIMEOUT, find_devices_inner())
        .await
        .map_err(|_| MapError::Timeout)?
}

async fn find_devices_inner() -> Result<Vec<MapDevice>> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
    let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.await?;
    let mut out = Vec::new();
    for info in infos {
        let id = info.Id()?;
        let Ok(device) = BluetoothDevice::FromIdAsync(&id)?.await else {
            continue;
        };
        let services = device
            .GetRfcommServicesForIdWithCacheModeAsync(&service_id(MAS_UUID)?, BluetoothCacheMode::Cached)?
            .await?;
        if services.Services()?.Size()? > 0 {
            out.push(MapDevice {
                id: id.to_string(),
                name: device.Name()?.to_string(),
            });
        }
    }
    Ok(out)
}

/// One OBEX session over RFCOMM: CONNECT with a target, then request/response.
pub(super) struct ObexLink {
    socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    connection_id: u32,
    max_packet: usize,
}

impl ObexLink {
    /// Open RFCOMM to `service_uuid` and run OBEX CONNECT. A fresh grant typically
    /// answers the first CONNECT with Forbidden, which is what makes iOS show its
    /// consent toggle; the caller maps that to the right consent error.
    pub(super) async fn connect(
        device_id: &str,
        service_uuid: u128,
        target: &[u8; 16],
        forbidden: MapError,
    ) -> Result<Self> {
        Self::connect_with(device_id, service_uuid, target, forbidden, false, &[]).await
    }

    /// `cached_first`: try Windows' cached service record (no SDP query over the air) before
    /// asking the phone afresh. Used for message access, where a reconnect can stand between the
    /// user and a text going out; a stale record just fails to connect and falls back.
    pub(super) async fn connect_with(
        device_id: &str,
        service_uuid: u128,
        target: &[u8; 16],
        forbidden: MapError,
        cached_first: bool,
        extra: &[Header],
    ) -> Result<Self> {
        tokio::time::timeout(
            CONNECT_TIMEOUT,
            Self::connect_inner(device_id, service_uuid, target, forbidden, cached_first, extra),
        )
        .await
        .map_err(|_| MapError::Timeout)?
    }

    /// An RFCOMM socket to `service_uuid` on `device`, its channel found with `mode`.
    async fn open_socket(
        device: &BluetoothDevice,
        service_uuid: u128,
        mode: BluetoothCacheMode,
    ) -> Result<StreamSocket> {
        let services = device
            .GetRfcommServicesForIdWithCacheModeAsync(&service_id(service_uuid)?, mode)?
            .await?
            .Services()?;
        if services.Size()? == 0 {
            return Err(MapError::NoService);
        }
        let service = services.GetAt(0)?;
        let socket = StreamSocket::new()?;
        let connected = socket
            .ConnectWithProtectionLevelAsync(
                &service.ConnectionHostName()?,
                &service.ConnectionServiceName()?,
                SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
            )?
            .await;
        if let Err(e) = connected {
            let _ = socket.Close();
            return Err(e.into());
        }
        Ok(socket)
    }

    async fn connect_inner(
        device_id: &str,
        service_uuid: u128,
        target: &[u8; 16],
        forbidden: MapError,
        cached_first: bool,
        extra: &[Header],
    ) -> Result<Self> {
        let device = BluetoothDevice::FromIdAsync(&HSTRING::from(device_id))?.await?;
        let cached = if cached_first {
            match Self::open_socket(&device, service_uuid, BluetoothCacheMode::Cached).await {
                Ok(socket) => Some(socket),
                Err(e) => {
                    log::debug!("the cached service record didn't connect ({e}); asking the phone afresh");
                    None
                }
            }
        } else {
            None
        };
        let socket = match cached {
            Some(socket) => socket,
            None => Self::open_socket(&device, service_uuid, BluetoothCacheMode::Uncached).await?,
        };
        let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
        let mut link = Self {
            socket,
            reader,
            writer,
            connection_id: 0,
            max_packet: 255,
        };
        let resp = link.exchange(&obex::connect_with(target, extra), true).await?;
        if !resp.is_success() {
            let ids: Vec<String> = resp.headers.iter().map(|h| format!("{:#04x}", h.id())).collect();
            log::info!(
                "OBEX CONNECT refused: code {:#04x}, headers [{}]",
                resp.code,
                ids.join(", ")
            );
        }
        match resp.code {
            obex::RSP_SUCCESS => {}
            // iOS answers Forbidden or (for PBAP) Unauthorized while the user hasn't
            // allowed this PC; both mean "turn the switch on", not a protocol error.
            obex::RSP_FORBIDDEN | obex::RSP_UNAUTHORIZED => return Err(forbidden),
            code => return Err(MapError::Obex { op: "CONNECT", code }),
        }
        link.connection_id = resp.connection_id().unwrap_or(0);
        link.max_packet = resp.max_packet.unwrap_or(255).min(obex::MAX_PACKET) as usize;
        Ok(link)
    }

    pub(super) fn conn(&self) -> Header {
        Header::connection_id(self.connection_id)
    }

    async fn send(&mut self, packet: &[u8]) -> Result<()> {
        self.writer.WriteBytes(packet)?;
        self.writer.StoreAsync()?.await?;
        Ok(())
    }

    async fn read_packet(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        loop {
            let need = match obex::packet_len(&buf) {
                Some(len) if buf.len() >= len => return Ok(buf),
                Some(len) => len - buf.len(),
                None => 3 - buf.len(),
            };
            let got = self.reader.LoadAsync(need as u32)?.await?;
            if got == 0 {
                return Err(MapError::Closed);
            }
            let mut chunk = vec![0; got as usize];
            self.reader.ReadBytes(&mut chunk)?;
            buf.extend_from_slice(&chunk);
        }
    }

    /// Send a request and read its response, bounded as a whole: a stalled write
    /// (full socket buffer, half-dead link) must time out just like a silent peer.
    pub(super) async fn exchange(&mut self, packet: &[u8], connect: bool) -> Result<Response> {
        let raw = tokio::time::timeout(RESPONSE_TIMEOUT, async {
            self.send(packet).await?;
            self.read_packet().await
        })
        .await
        .map_err(|_| MapError::Timeout)??;
        Ok(obex::parse_response(&raw, connect)?)
    }

    pub(super) async fn set_path(&mut self, name: Option<&str>) -> Result<()> {
        let headers = [self.conn(), Header::Name(name.map(str::to_string))];
        let resp = self
            .exchange(
                &obex::request(obex::OP_SETPATH, &[obex::SETPATH_DONT_CREATE, 0], &headers),
                false,
            )
            .await?;
        if !resp.is_success() {
            return Err(MapError::Obex {
                op: "SETPATH",
                code: resp.code,
            });
        }
        Ok(())
    }

    /// OBEX GET, following Continue responses until the object is complete.
    pub(super) async fn get(&mut self, op: &'static str, headers: Vec<Header>) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        let mut packet = obex::request(obex::OP_GET_FINAL, &[], &headers);
        loop {
            let resp = self.exchange(&packet, false).await?;
            body.extend(resp.body());
            match resp.code {
                obex::RSP_SUCCESS => return Ok(body),
                obex::RSP_CONTINUE => packet = obex::request(obex::OP_GET_FINAL, &[], &[self.conn()]),
                code => return Err(MapError::Obex { op, code }),
            }
        }
    }

    /// OBEX DISCONNECT, then close the socket (`Drop` does that, once).
    pub(super) async fn disconnect(mut self) {
        let packet = obex::request(obex::OP_DISCONNECT, &[], &[self.conn()]);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.exchange(&packet, false)).await;
    }
}

/// Every way a link ends closes its RFCOMM socket: a clean `disconnect`, an error, or a caller's
/// timeout (or abort) dropping a pull mid-transfer, which would otherwise leave the phone's single
/// PBAP (or MAP) connection open until Windows noticed.
impl Drop for ObexLink {
    fn drop(&mut self) {
        let _ = self.socket.Close();
    }
}

/// A MAP client session with the iPhone's Message Access Server.
pub struct MapSession {
    pub(super) link: ObexLink,
    /// The session's current folder is `/telecom/msg` (nothing tug does on a session leaves it),
    /// so a send or listing needn't walk there again: three round trips saved per send.
    at_msg: bool,
}

/// The SETPATH steps from wherever the session is to `/telecom/msg`: none once it's there (where
/// every listing and send leaves it), otherwise from root (SETPATH is relative). `None` is root.
/// Pure, so "a send on an open session is just the push" is unit-tested.
fn path_to_msg(at_msg: bool) -> &'static [Option<&'static str>] {
    if at_msg {
        &[]
    } else {
        &[None, Some("telecom"), Some("msg")]
    }
}

impl MapSession {
    pub async fn connect(device_id: &str) -> Result<Self> {
        let link = ObexLink::connect_with(device_id, MAS_UUID, &MAS_TARGET, MapError::Consent, true, &[]).await?;
        log::info!(
            "MAP session open (connection id {}, max packet {})",
            link.connection_id,
            link.max_packet
        );
        Ok(Self { link, at_msg: false })
    }

    /// Go to `/telecom/msg` (already there after the first listing: no round trips).
    pub(super) async fn goto_msg(&mut self) -> Result<()> {
        let steps = path_to_msg(self.at_msg);
        // Wherever a failed step leaves it, the next walk starts again from root.
        self.at_msg = false;
        for step in steps {
            self.link.set_path(*step).await?;
        }
        self.at_msg = true;
        Ok(())
    }

    /// Newest messages in `folder` (e.g. "inbox"), at most `max`.
    /// Newest first: `max` messages, skipping the newest `offset`.
    pub async fn list(&mut self, folder: &str, max: u16, offset: u16) -> Result<Vec<ListedMessage>> {
        self.goto_msg().await?;
        let headers = vec![
            self.link.conn(),
            Header::type_("x-bt/MAP-msg-listing"),
            Header::Name(Some(folder.to_string())),
            obex::app_params(&[
                (AP_MAX_LIST_COUNT, &max.to_be_bytes()),
                (AP_LIST_START_OFFSET, &offset.to_be_bytes()),
            ]),
        ];
        let xml = self.link.get("GetMessagesListing", headers).await?;
        // A subject that isn't UTF-8 reads as Windows-1252 rather than turning into U+FFFD.
        Ok(listing::parse(&charset::decode_line(&xml)))
    }

    pub async fn get_message(&mut self, handle: &str) -> Result<BMessage> {
        let headers = vec![
            self.link.conn(),
            Header::type_("x-bt/message"),
            Header::Name(Some(handle.to_string())),
            obex::app_params(&[(AP_ATTACHMENT, &[0]), (AP_CHARSET, &[CHARSET_UTF8])]),
        ];
        let raw = self.link.get("GetMessage", headers).await?;
        // Decoded from the bytes (never lossily as a whole), so a body in another encoding still
        // reads as words. When that took a fallback, log how and the bytes' shape, never the text.
        let message = bmessage::parse_bytes(&raw);
        if message.decoding.is_fallback() {
            log::debug!(
                "message {handle}: text decoded as {:?} ({})",
                message.decoding,
                message.body_pattern
            );
        }
        if let Some(shape) = bmessage::unknown_attachment_shape(&message.body) {
            if !ATTACHMENT_SHAPE_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::info!(
                    "a text looks like an attachment line in another language (shape \"{shape}\"); shown as text"
                );
            }
        }
        Ok(message)
    }

    /// Ask the phone to refresh its inbox view before listing.
    pub async fn update_inbox(&mut self) -> Result<()> {
        let headers = [
            self.link.conn(),
            Header::type_("x-bt/MAP-messageUpdate"),
            Header::Bytes(obex::HI_END_OF_BODY, vec![0x30]),
        ];
        let resp = self
            .link
            .exchange(&obex::request(obex::OP_PUT_FINAL, &[], &headers), false)
            .await?;
        if !resp.is_success() {
            return Err(MapError::Obex {
                op: "UpdateInbox",
                code: resp.code,
            });
        }
        Ok(())
    }

    /// SetMessageStatus: mark a message read (or unread) on the phone. Fetching a
    /// message with GetMessage doesn't change its read state; only this does.
    pub async fn set_read(&mut self, handle: &str, read: bool) -> Result<()> {
        let headers = [
            self.link.conn(),
            Header::Name(Some(handle.to_string())),
            Header::type_("x-bt/messageStatus"),
            obex::app_params(&[(AP_STATUS_INDICATOR, &[STATUS_READ]), (AP_STATUS_VALUE, &[read as u8])]),
            Header::Bytes(obex::HI_END_OF_BODY, vec![0x30]),
        ];
        let resp = self
            .link
            .exchange(&obex::request(obex::OP_PUT_FINAL, &[], &headers), false)
            .await?;
        if !resp.is_success() {
            return Err(MapError::Obex {
                op: "SetMessageStatus",
                code: resp.code,
            });
        }
        Ok(())
    }

    /// SetNotificationRegistration: ask the phone to connect to our Message Notification
    /// Server and push event reports (`map::mns_event`), or to stop. Nothing calls this
    /// until tug hosts an MNS; turning it on without one leaves the phone retrying.
    pub async fn set_notification_registration(&mut self, on: bool) -> Result<()> {
        let packet = notification_registration_request(self.link.connection_id, on);
        let resp = self.link.exchange(&packet, false).await?;
        if !resp.is_success() {
            return Err(MapError::Obex {
                op: "SetNotificationRegistration",
                code: resp.code,
            });
        }
        Ok(())
    }

    /// PushMessage to the outbox. Success means *accepted by the iPhone*; it does
    /// not confirm carrier delivery. Returns the handle iOS assigned, if any.
    pub async fn push_message(
        &mut self,
        recipient: &str,
        text: &str,
    ) -> std::result::Result<Option<String>, PushError> {
        let early = |error| PushError {
            error,
            maybe_taken: false,
        };
        self.goto_msg().await.map_err(early)?;
        let object = bmessage::compose(recipient, text);
        let first = vec![
            self.link.conn(),
            Header::type_("x-bt/message"),
            Header::Name(Some("outbox".into())),
            obex::app_params(&[(AP_CHARSET, &[CHARSET_UTF8])]),
        ];
        // Room for body per packet: max packet minus opcode/length and header overhead.
        let room = self.link.max_packet.saturating_sub(64).max(32);
        let chunks: Vec<&[u8]> = object.chunks(room).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            let mut headers = if i == 0 { first.clone() } else { vec![self.link.conn()] };
            headers.push(Header::Bytes(
                if last { obex::HI_END_OF_BODY } else { obex::HI_BODY },
                chunk.to_vec(),
            ));
            let opcode = if last { obex::OP_PUT_FINAL } else { obex::OP_PUT };
            // Before the final packet the phone can't have the whole text; once it's out, a lost
            // answer leaves it unknown whether the phone took it.
            let resp = self
                .link
                .exchange(&obex::request(opcode, &[], &headers), false)
                .await
                .map_err(|error| PushError {
                    error,
                    maybe_taken: last,
                })?;
            let expected = if last { obex::RSP_SUCCESS } else { obex::RSP_CONTINUE };
            if resp.code != expected {
                // A refusal is a definite answer: not sent.
                return Err(early(MapError::Obex {
                    op: "PushMessage",
                    code: resp.code,
                }));
            }
            if last {
                return Ok(resp.name().map(str::to_string));
            }
        }
        Ok(None)
    }

    pub async fn disconnect(self) {
        self.link.disconnect().await;
    }
}

/// The SetNotificationRegistration PUT, kept pure so its bytes can be tested. iOS exposes a
/// single MAS, instance 0. The End-of-Body carries MAP's 0x30 filler byte, like the spec, BlueZ,
/// and tug's own UpdateInbox / SetMessageStatus PUTs, which the test iPhone already accepts
/// (another iOS client sends it empty; try that if iOS ever refuses the registration).
fn notification_registration_request(connection_id: u32, on: bool) -> Vec<u8> {
    let headers = [
        Header::connection_id(connection_id),
        Header::type_("x-bt/MAP-NotificationRegistration"),
        obex::app_params(&[(AP_NOTIFICATION_STATUS, &[on as u8]), (AP_MAS_INSTANCE_ID, &[0])]),
        Header::Bytes(obex::HI_END_OF_BODY, vec![0x30]),
    ];
    obex::request(obex::OP_PUT_FINAL, &[], &headers)
}

/// Pull the iPhone's contacts over PBAP (`telecom/pb.vcf`): names and numbers, plus a photo per
/// contact when `with_photos` (the phone inlines one when it has it). Photos make the pull far
/// slower — on a test phone ~60 s with photos versus ~14 s without — so the regular (fast) sync
/// passes `false` and a separate background pass passes `true`; see `map::service`.
pub async fn pull_contacts(device_id: &str, with_photos: bool) -> Result<Vec<PhonebookEntry>> {
    let mut link = ObexLink::connect(device_id, PSE_UUID, &PBAP_TARGET, MapError::ContactsConsent).await?;
    let result = pull_phonebook(&mut link, with_photos).await;
    link.disconnect().await;
    result
}

async fn pull_phonebook(link: &mut ObexLink, with_photos: bool) -> Result<Vec<PhonebookEntry>> {
    let properties = if with_photos {
        PB_CONTACT_PROPERTIES
    } else {
        PB_PROPERTIES
    };
    let headers = vec![
        link.conn(),
        Header::type_("x-bt/phonebook"),
        Header::Name(Some("telecom/pb.vcf".into())),
        obex::app_params(&[
            (PB_FORMAT, &[PB_FORMAT_VCARD30]),
            (PB_PROPERTY_SELECTOR, &properties.to_be_bytes()),
            (PB_MAX_LIST_COUNT, &u16::MAX.to_be_bytes()),
        ]),
    ];
    let raw = link.get("PullPhoneBook", headers).await?;
    Ok(parse_phonebook(
        &raw,
        if with_photos { "contacts+photos" } else { "contacts" },
    ))
}

fn parse_phonebook(raw: &[u8], kind: &'static str) -> Vec<PhonebookEntry> {
    let text = String::from_utf8_lossy(raw).into_owned();
    // Bluetooth inventory: which vCard property NAMES this pull carried (counts only).
    crate::bt_inventory::record_vcard_fields(kind, &text);
    vcard::parse(&text)
}

/// What a contacts check found (`check_contacts`).
#[derive(Debug)]
pub enum PhonebookCheck {
    /// Settled without the names (`contacts_watch::CheckAnswer::Empty` or `Unchanged`).
    Settled { empty: bool, fingerprint: Fingerprint },
    /// The names, pulled because the check said so (or the phone sent them anyway).
    Entries {
        entries: Vec<PhonebookEntry>,
        fingerprint: Fingerprint,
    },
}

/// The contacts check: one PBAP connection that first asks only for the phonebook's size and
/// version counters (PullPhoneBook with MaxListCount 0: one short round trip, no names), and pulls
/// the names (no photos) only when `contacts_watch::after_check` says that doesn't settle it. The
/// CONNECT offers PBAP 1.2's database identifier and folder version counters; a phone that refuses
/// that CONNECT gets a plain one, and a phone that sends no counters, refuses the size question or
/// sends the whole phonebook anyway still gets the right answer.
pub async fn check_contacts(
    device_id: &str,
    last: Option<Fingerprint>,
    zero_trusted: bool,
    routine: bool,
) -> Result<PhonebookCheck> {
    use super::contacts_watch::{after_check, CheckAnswer};
    let features = [obex::app_params(&[(
        PB_SUPPORTED_FEATURES,
        &PCE_FEATURES.to_be_bytes(),
    )])];
    let consent = || MapError::ContactsConsent;
    let mut link = match ObexLink::connect_with(device_id, PSE_UUID, &PBAP_TARGET, consent(), false, &features).await {
        Ok(link) => link,
        Err(MapError::Obex { op: "CONNECT", code }) => {
            log::info!("the iPhone refused a PBAP 1.2 connect (OBEX {code:#04x}); connecting without counters");
            ObexLink::connect(device_id, PSE_UUID, &PBAP_TARGET, consent()).await?
        }
        Err(e) => return Err(e),
    };
    let result = async {
        let (fingerprint, body) = match phonebook_fingerprint(&mut link).await {
            Ok(answer) => answer,
            // Refused (a phone that won't do size-only pulls): pull the names instead.
            Err(MapError::Obex { code, .. }) => {
                log::debug!("phonebook size check refused (OBEX {code:#04x}); pulling the names");
                (Fingerprint::default(), Vec::new())
            }
            Err(e) => return Err(e),
        };
        if !body.is_empty() {
            // It ignored MaxListCount 0 and sent the phonebook: use it rather than ask again.
            return Ok(PhonebookCheck::Entries {
                entries: parse_phonebook(&body, "contacts"),
                fingerprint,
            });
        }
        match after_check(&fingerprint, last.as_ref(), zero_trusted, routine) {
            CheckAnswer::Empty => Ok(PhonebookCheck::Settled {
                empty: true,
                fingerprint,
            }),
            CheckAnswer::Unchanged => Ok(PhonebookCheck::Settled {
                empty: false,
                fingerprint,
            }),
            CheckAnswer::Pull => Ok(PhonebookCheck::Entries {
                entries: pull_phonebook(&mut link, false).await?,
                fingerprint,
            }),
        }
    }
    .await;
    link.disconnect().await;
    result
}

/// PullPhoneBook with MaxListCount 0: what the phone says about its phonebook (size, version
/// counters, database identifier) and any body it sent regardless.
async fn phonebook_fingerprint(link: &mut ObexLink) -> Result<(Fingerprint, Vec<u8>)> {
    let headers = vec![
        link.conn(),
        Header::type_("x-bt/phonebook"),
        Header::Name(Some("telecom/pb.vcf".into())),
        obex::app_params(&[
            (PB_FORMAT, &[PB_FORMAT_VCARD30]),
            (PB_PROPERTY_SELECTOR, &PB_PROPERTIES.to_be_bytes()),
            (PB_MAX_LIST_COUNT, &0u16.to_be_bytes()),
        ]),
    ];
    let mut body = Vec::new();
    let mut fingerprint = Fingerprint::default();
    let mut packet = obex::request(obex::OP_GET_FINAL, &[], &headers);
    loop {
        let resp = link.exchange(&packet, false).await?;
        read_fingerprint(&resp, &mut fingerprint);
        body.extend(resp.body());
        match resp.code {
            obex::RSP_SUCCESS => return Ok((fingerprint, body)),
            obex::RSP_CONTINUE => packet = obex::request(obex::OP_GET_FINAL, &[], &[link.conn()]),
            code => {
                return Err(MapError::Obex {
                    op: "PullPhoneBook size",
                    code,
                })
            }
        }
    }
}

/// Fill in what this response carries: PhonebookSize (two bytes), the primary and secondary
/// version counters and the database identifier (16 bytes each). Malformed values are ignored.
fn read_fingerprint(resp: &Response, fp: &mut Fingerprint) {
    let bytes16 = |tag| resp.app_param(tag).and_then(|v| <[u8; 16]>::try_from(v).ok());
    if let Some(size) = resp
        .app_param(PB_PHONEBOOK_SIZE)
        .and_then(|v| <[u8; 2]>::try_from(v).ok())
    {
        fp.size = Some(u16::from_be_bytes(size));
    }
    fp.primary = bytes16(PB_PRIMARY_COUNTER).or(fp.primary);
    fp.secondary = bytes16(PB_SECONDARY_COUNTER).or(fp.secondary);
    fp.database = bytes16(PB_DATABASE_ID).or(fp.database);
}

/// Pull the newest `max` calls over PBAP: the combined list (`telecom/cch.vcf`), or, from a
/// phone that doesn't keep one, incoming, outgoing and missed merged. Gated by the same Sync
/// Contacts switch as the phonebook.
pub async fn pull_call_history(device_id: &str, max: u16) -> Result<Vec<CallRecord>> {
    let mut link = ObexLink::connect(device_id, PSE_UUID, &PBAP_TARGET, MapError::ContactsConsent).await?;
    let result = async {
        match pull_calls(&mut link, "cch", max).await {
            Ok(raw) => Ok(calls::parse(&raw, None)),
            // Not Found / Bad Request / Not Implemented: no combined list on this phone.
            Err(MapError::Obex { code, .. }) => {
                log::info!("no combined call history (OBEX {code:#04x}); reading the three lists");
                let mut lists = Vec::new();
                for (name, direction) in [
                    ("ich", CallDirection::Incoming),
                    ("och", CallDirection::Outgoing),
                    ("mch", CallDirection::Missed),
                ] {
                    let raw = pull_calls(&mut link, name, max).await?;
                    lists.push(calls::parse(&raw, Some(direction)));
                }
                Ok(calls::merge(lists, max as usize))
            }
            Err(e) => Err(e),
        }
    }
    .await;
    link.disconnect().await;
    result
}

async fn pull_calls(link: &mut ObexLink, list: &str, max: u16) -> Result<String> {
    let headers = vec![
        link.conn(),
        Header::type_("x-bt/phonebook"),
        Header::Name(Some(format!("telecom/{list}.vcf"))),
        obex::app_params(&[
            (PB_FORMAT, &[PB_FORMAT_VCARD30]),
            (PB_PROPERTY_SELECTOR, &PB_CALL_PROPERTIES.to_be_bytes()),
            (PB_MAX_LIST_COUNT, &max.to_be_bytes()),
        ]),
    ];
    let raw = link.get("PullPhoneBook", headers).await?;
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_registration_bytes() {
        let p = notification_registration_request(0x0000_0001, true);
        let ty = b"x-bt/MAP-NotificationRegistration\0";
        let mut expected = vec![obex::OP_PUT_FINAL, 0, 0];
        expected.extend_from_slice(&[obex::HI_CONNECTION_ID, 0, 0, 0, 1]);
        expected.extend_from_slice(&[obex::HI_TYPE, 0, (ty.len() + 3) as u8]);
        expected.extend_from_slice(ty);
        expected.extend_from_slice(&[obex::HI_APP_PARAMS, 0, 9, 0x0E, 0x01, 0x01, 0x0F, 0x01, 0x00]);
        expected.extend_from_slice(&[obex::HI_END_OF_BODY, 0, 4, 0x30]);
        let len = expected.len() as u16;
        expected[1..3].copy_from_slice(&len.to_be_bytes());
        assert_eq!(p, expected);
    }

    #[test]
    fn a_send_on_an_open_session_walks_no_folders() {
        // The first listing on a session walks root → telecom → msg; after that a send is the
        // PushMessage alone, with nothing before it.
        assert_eq!(path_to_msg(false), &[None, Some("telecom"), Some("msg")]);
        assert!(path_to_msg(true).is_empty());
    }

    #[test]
    fn reads_what_a_size_only_answer_says_about_the_phonebook() {
        let answer = |params: &[(u8, &[u8])]| Response {
            code: obex::RSP_SUCCESS,
            max_packet: None,
            headers: vec![obex::app_params(params)],
        };
        let mut fp = Fingerprint::default();
        read_fingerprint(
            &answer(&[
                (0x08, &[0x01, 0x2C]),
                (0x0A, &[1; 16]),
                (0x0B, &[2; 16]),
                (0x0D, &[3; 16]),
            ]),
            &mut fp,
        );
        assert_eq!(
            fp,
            Fingerprint {
                size: Some(300),
                primary: Some([1; 16]),
                secondary: Some([2; 16]),
                database: Some([3; 16]),
            }
        );
        // A phone without PBAP 1.2 sends only the size; malformed values are ignored.
        let mut fp = Fingerprint::default();
        read_fingerprint(&answer(&[(0x08, &[0, 7]), (0x0B, &[2; 15])]), &mut fp);
        assert_eq!(fp.size, Some(7));
        assert!(!fp.has_counters());
        let mut fp = Fingerprint::default();
        read_fingerprint(&answer(&[(0x08, &[7])]), &mut fp);
        assert_eq!(fp, Fingerprint::default(), "a one-byte size is no size");
        // Spread over packets: each part is kept once seen.
        let mut fp = Fingerprint::default();
        read_fingerprint(&answer(&[(0x0D, &[3; 16])]), &mut fp);
        read_fingerprint(&answer(&[(0x08, &[0, 1])]), &mut fp);
        assert_eq!((fp.size, fp.database), (Some(1), Some([3; 16])));
    }

    #[test]
    fn the_pbap_connect_offers_counters_and_nothing_that_changes_a_pull() {
        assert_eq!(
            PCE_FEATURES, 0x0F,
            "download, browsing, database identifier, folder version counters"
        );
        let p = obex::connect_with(
            &PBAP_TARGET,
            &[obex::app_params(&[(
                PB_SUPPORTED_FEATURES,
                &PCE_FEATURES.to_be_bytes(),
            )])],
        );
        let r = obex::parse_request(&p).unwrap();
        assert_eq!(r.app_param(0x10), Some(&[0, 0, 0, 0x0F][..]));
    }

    #[test]
    fn notification_registration_off_and_parsed_back() {
        let r = obex::parse_request(&notification_registration_request(42, false)).unwrap();
        assert_eq!((r.opcode, r.is_final), (obex::OP_PUT, true));
        assert_eq!(r.connection_id(), Some(42));
        assert_eq!(r.type_(), Some("x-bt/MAP-NotificationRegistration"));
        assert_eq!(r.app_param(AP_NOTIFICATION_STATUS), Some(&[0u8][..]));
        assert_eq!(r.app_param(AP_MAS_INSTANCE_ID), Some(&[0u8][..]));
        assert!(r.has_end_of_body());
        assert_eq!(r.body(), &[0x30][..]);
    }
}
