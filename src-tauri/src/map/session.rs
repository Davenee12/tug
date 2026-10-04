//! A MAP client session with the iPhone's Message Access Server, over a WinRT
//! RFCOMM `StreamSocket`. WinRT resolves the MAS RFCOMM channel from SDP for us.

use std::time::Duration;

use thiserror::Error;
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::{SocketProtectionLevel, StreamSocket};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

use super::bmessage::{self, BMessage};
use super::listing::{self, ListedMessage};
use super::obex::{self, Header, ObexError, Response};

/// Message Access Server service class (SDP).
pub const MAS_UUID: u128 = 0x0000_1132_0000_1000_8000_0080_5F9B_34FB;
/// OBEX Target for MAS sessions.
pub const MAS_TARGET: [u8; 16] = [
    0xBB, 0x58, 0x2B, 0x40, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

const OP_PUT: u8 = 0x02;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

// MAP application parameter tags.
const AP_MAX_LIST_COUNT: u8 = 0x01;
const AP_ATTACHMENT: u8 = 0x0A;
const AP_CHARSET: u8 = 0x14;
const CHARSET_UTF8: u8 = 0x01;

#[derive(Debug, Error)]
pub enum MapError {
    #[error("no paired device offers Bluetooth message access")]
    NoDevice,
    #[error("the iPhone isn't offering message access over Bluetooth")]
    NoService,
    #[error(
        "the iPhone refused message access; turn on Show Notifications for this PC under Settings › Bluetooth › ⓘ"
    )]
    Consent,
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

fn mas_service_id() -> windows::core::Result<RfcommServiceId> {
    RfcommServiceId::FromUuid(GUID::from_u128(MAS_UUID))
}

/// Paired Classic devices that advertise a Message Access Server.
pub async fn find_devices() -> Result<Vec<MapDevice>> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
    let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.await?;
    let mut out = Vec::new();
    for info in infos {
        let id = info.Id()?;
        let Ok(device) = BluetoothDevice::FromIdAsync(&id)?.await else {
            continue;
        };
        let services = device
            .GetRfcommServicesForIdWithCacheModeAsync(&mas_service_id()?, BluetoothCacheMode::Cached)?
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

pub struct MapSession {
    socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    connection_id: u32,
    max_packet: usize,
}

impl MapSession {
    /// Open RFCOMM to the MAS and run OBEX CONNECT. A fresh pairing typically
    /// answers the first CONNECT with Forbidden, which is what makes iOS show
    /// its consent toggle; retry after the user enables it.
    pub async fn connect(device_id: &str) -> Result<Self> {
        let device = BluetoothDevice::FromIdAsync(&HSTRING::from(device_id))?.await?;
        let services = device
            .GetRfcommServicesForIdWithCacheModeAsync(&mas_service_id()?, BluetoothCacheMode::Uncached)?
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
        let mut session = Self {
            socket,
            reader,
            writer,
            connection_id: 0,
            max_packet: 255,
        };
        let resp = session.exchange(&obex::connect(&MAS_TARGET), true).await?;
        match resp.code {
            obex::RSP_SUCCESS => {}
            obex::RSP_FORBIDDEN => return Err(MapError::Consent),
            code => return Err(MapError::Obex { op: "CONNECT", code }),
        }
        session.connection_id = resp.connection_id().unwrap_or(0);
        session.max_packet = resp.max_packet.unwrap_or(255).min(obex::MAX_PACKET) as usize;
        log::info!(
            "MAP session open (connection id {}, max packet {})",
            session.connection_id,
            session.max_packet
        );
        Ok(session)
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

    async fn exchange(&mut self, packet: &[u8], connect: bool) -> Result<Response> {
        self.send(packet).await?;
        let raw = tokio::time::timeout(RESPONSE_TIMEOUT, self.read_packet())
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

    /// SETPATH is relative, so always start from root: `/telecom/msg`.
    async fn goto_msg(&mut self) -> Result<()> {
        self.set_path(None).await?;
        self.set_path(Some("telecom")).await?;
        self.set_path(Some("msg")).await
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

    /// Newest messages in `folder` (e.g. "inbox"), at most `max`.
    pub async fn list(&mut self, folder: &str, max: u16) -> Result<Vec<ListedMessage>> {
        self.goto_msg().await?;
        let headers = vec![
            self.conn(),
            Header::type_("x-bt/MAP-msg-listing"),
            Header::Name(Some(folder.to_string())),
            obex::app_params(&[(AP_MAX_LIST_COUNT, &max.to_be_bytes())]),
        ];
        let xml = self.get("GetMessagesListing", headers).await?;
        Ok(listing::parse(&String::from_utf8_lossy(&xml)))
    }

    pub async fn get_message(&mut self, handle: &str) -> Result<BMessage> {
        let headers = vec![
            self.conn(),
            Header::type_("x-bt/message"),
            Header::Name(Some(handle.to_string())),
            obex::app_params(&[(AP_ATTACHMENT, &[0]), (AP_CHARSET, &[CHARSET_UTF8])]),
        ];
        let raw = self.get("GetMessage", headers).await?;
        Ok(bmessage::parse(&String::from_utf8_lossy(&raw)))
    }

    /// Ask the phone to refresh its inbox view before listing.
    pub async fn update_inbox(&mut self) -> Result<()> {
        let headers = [
            self.conn(),
            Header::type_("x-bt/MAP-messageUpdate"),
            Header::Bytes(obex::HI_END_OF_BODY, vec![0x30]),
        ];
        let resp = self
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

    /// PushMessage to the outbox. Success means *accepted by the iPhone*; it does
    /// not confirm carrier delivery. Returns the handle iOS assigned, if any.
    pub async fn push_message(&mut self, recipient: &str, text: &str) -> Result<Option<String>> {
        self.goto_msg().await?;
        let object = bmessage::compose(recipient, text);
        let first = vec![
            self.conn(),
            Header::type_("x-bt/message"),
            Header::Name(Some("outbox".into())),
            obex::app_params(&[(AP_CHARSET, &[CHARSET_UTF8])]),
        ];
        // Room for body per packet: max packet minus opcode/length and header overhead.
        let room = self.max_packet.saturating_sub(64).max(32);
        let chunks: Vec<&[u8]> = object.chunks(room).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            let mut headers = if i == 0 { first.clone() } else { vec![self.conn()] };
            headers.push(Header::Bytes(
                if last { obex::HI_END_OF_BODY } else { obex::HI_BODY },
                chunk.to_vec(),
            ));
            let opcode = if last { obex::OP_PUT_FINAL } else { OP_PUT };
            let resp = self.exchange(&obex::request(opcode, &[], &headers), false).await?;
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

    pub async fn disconnect(mut self) {
        let packet = obex::request(obex::OP_DISCONNECT, &[], &[self.conn()]);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.exchange(&packet, false)).await;
        let _ = self.socket.Close();
    }
}
