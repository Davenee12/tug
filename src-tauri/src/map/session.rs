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

const OP_PUT: u8 = 0x02;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
/// Device lookup + RFCOMM connect + OBEX CONNECT, end to end.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

// MAP application parameter tags.
const AP_MAX_LIST_COUNT: u8 = 0x01;
const AP_LIST_START_OFFSET: u8 = 0x02;
const AP_ATTACHMENT: u8 = 0x0A;
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
/// PropertySelector bits: VERSION, FN, N, TEL.
const PB_PROPERTIES: u64 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 7);
/// The same plus X-IRMC-CALL-DATETIME (bit 28), which carries a call's direction and time.
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

#[derive(Debug, Clone)]
pub struct MapDevice {
    pub id: String,
    pub name: String,
}

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
struct ObexLink {
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
    async fn connect(device_id: &str, service_uuid: u128, target: &[u8; 16], forbidden: MapError) -> Result<Self> {
        tokio::time::timeout(
            CONNECT_TIMEOUT,
            Self::connect_inner(device_id, service_uuid, target, forbidden),
        )
        .await
        .map_err(|_| MapError::Timeout)?
    }

    async fn connect_inner(
        device_id: &str,
        service_uuid: u128,
        target: &[u8; 16],
        forbidden: MapError,
    ) -> Result<Self> {
        let device = BluetoothDevice::FromIdAsync(&HSTRING::from(device_id))?.await?;
        let services = device
            .GetRfcommServicesForIdWithCacheModeAsync(&service_id(service_uuid)?, BluetoothCacheMode::Uncached)?
            .await?
            .Services()?;
        if services.Size()? == 0 {
            return Err(MapError::NoService);
        }
        let service = services.GetAt(0)?;
        let socket = StreamSocket::new()?;
        socket
            .ConnectWithProtectionLevelAsync(
                &service.ConnectionHostName()?,
                &service.ConnectionServiceName()?,
                SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
            )?
            .await?;
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
        let resp = link.exchange(&obex::connect(target), true).await?;
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

    fn conn(&self) -> Header {
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
    async fn exchange(&mut self, packet: &[u8], connect: bool) -> Result<Response> {
        let raw = tokio::time::timeout(RESPONSE_TIMEOUT, async {
            self.send(packet).await?;
            self.read_packet().await
        })
        .await
        .map_err(|_| MapError::Timeout)??;
        Ok(obex::parse_response(&raw, connect)?)
    }

    async fn set_path(&mut self, name: Option<&str>) -> Result<()> {
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
    async fn get(&mut self, op: &'static str, headers: Vec<Header>) -> Result<Vec<u8>> {
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

    async fn disconnect(mut self) {
        let packet = obex::request(obex::OP_DISCONNECT, &[], &[self.conn()]);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.exchange(&packet, false)).await;
        let _ = self.socket.Close();
    }
}

/// A MAP client session with the iPhone's Message Access Server.
pub struct MapSession {
    link: ObexLink,
}

impl MapSession {
    pub async fn connect(device_id: &str) -> Result<Self> {
        let link = ObexLink::connect(device_id, MAS_UUID, &MAS_TARGET, MapError::Consent).await?;
        log::info!(
            "MAP session open (connection id {}, max packet {})",
            link.connection_id,
            link.max_packet
        );
        Ok(Self { link })
    }

    /// SETPATH is relative, so always start from root: `/telecom/msg`.
    async fn goto_msg(&mut self) -> Result<()> {
        self.link.set_path(None).await?;
        self.link.set_path(Some("telecom")).await?;
        self.link.set_path(Some("msg")).await
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
        Ok(listing::parse(&String::from_utf8_lossy(&xml)))
    }

    pub async fn get_message(&mut self, handle: &str) -> Result<BMessage> {
        let headers = vec![
            self.link.conn(),
            Header::type_("x-bt/message"),
            Header::Name(Some(handle.to_string())),
            obex::app_params(&[(AP_ATTACHMENT, &[0]), (AP_CHARSET, &[CHARSET_UTF8])]),
        ];
        let raw = self.link.get("GetMessage", headers).await?;
        Ok(bmessage::parse(&String::from_utf8_lossy(&raw)))
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

    /// PushMessage to the outbox. Success means *accepted by the iPhone*; it does
    /// not confirm carrier delivery. Returns the handle iOS assigned, if any.
    pub async fn push_message(&mut self, recipient: &str, text: &str) -> Result<Option<String>> {
        self.goto_msg().await?;
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
            let opcode = if last { obex::OP_PUT_FINAL } else { OP_PUT };
            let resp = self.link.exchange(&obex::request(opcode, &[], &headers), false).await?;
            let expected = if last { obex::RSP_SUCCESS } else { obex::RSP_CONTINUE };
            if resp.code != expected {
                return Err(MapError::Obex {
                    op: "PushMessage",
                    code: resp.code,
                });
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

/// Pull the iPhone's contacts over PBAP (`telecom/pb.vcf`): names and numbers only.
pub async fn pull_contacts(device_id: &str) -> Result<Vec<PhonebookEntry>> {
    let mut link = ObexLink::connect(device_id, PSE_UUID, &PBAP_TARGET, MapError::ContactsConsent).await?;
    let headers = vec![
        link.conn(),
        Header::type_("x-bt/phonebook"),
        Header::Name(Some("telecom/pb.vcf".into())),
        obex::app_params(&[
            (PB_FORMAT, &[PB_FORMAT_VCARD30]),
            (PB_PROPERTY_SELECTOR, &PB_PROPERTIES.to_be_bytes()),
            (PB_MAX_LIST_COUNT, &u16::MAX.to_be_bytes()),
        ]),
    ];
    let result = link.get("PullPhoneBook", headers).await;
    link.disconnect().await;
    Ok(vcard::parse(&String::from_utf8_lossy(&result?)))
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
