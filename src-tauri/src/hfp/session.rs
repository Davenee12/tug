//! **Experimental, not verified on hardware.** The hands-free link to the iPhone over a WinRT
//! RFCOMM `StreamSocket`: connect to its Audio Gateway, set up the Service Level Connection,
//! optionally dial, then close. Protocol decisions live in `at`.

use std::time::Duration;

use thiserror::Error;
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice, BluetoothError};
use windows::Devices::Enumeration::DeviceAccessStatus;
use windows::Networking::Sockets::{SocketProtectionLevel, StreamSocket};
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};

use super::at::{self, CallProgress, Next, NumberError, Reply, Slc, SlcError};

/// Hands-Free Audio Gateway service class (SDP): the phone's side of HFP.
pub const HFP_AG_UUID: u128 = 0x0000_111F_0000_1000_8000_0080_5F9B_34FB;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// E_ACCESSDENIED: Windows won't let an app onto this service.
const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;

#[derive(Debug, Error)]
pub enum HfpError {
    #[error("the iPhone isn't offering hands-free calling to this PC")]
    NoService,
    #[error("Windows won't share the iPhone's hands-free link with tug (Windows or Phone Link is probably using it)")]
    Blocked,
    #[error("{0}")]
    Slc(#[from] SlcError),
    #[error("{0}")]
    Number(#[from] NumberError),
    #[error("the iPhone wouldn't dial ({0})")]
    DialRefused(String),
    #[error("the iPhone didn't answer in time")]
    Timeout,
    #[error("the connection closed")]
    Closed,
    #[error("Windows Bluetooth error: {} ({:#010x})", .0.message(), .0.code().0)]
    Win(#[from] windows::core::Error),
}

/// What happened, for logs and the probe.
#[derive(Debug, Default)]
pub struct Report {
    pub ag_features: Option<u32>,
    pub indicators: Vec<String>,
    /// Set when a number was dialed and the phone said OK to it.
    pub dialed: bool,
    pub progress: Vec<CallProgress>,
}

struct Link {
    socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    buf: String,
}

impl Link {
    async fn open(device_id: &str) -> Result<Self, HfpError> {
        let device = BluetoothDevice::FromIdAsync(&HSTRING::from(device_id))?.await?;
        let id = RfcommServiceId::FromUuid(GUID::from_u128(HFP_AG_UUID))?;
        let result = device
            .GetRfcommServicesForIdWithCacheModeAsync(&id, BluetoothCacheMode::Uncached)?
            .await?;
        let services = result.Services()?;
        if services.Size()? == 0 {
            let error = result.Error()?;
            log::info!("no hands-free service listed (Bluetooth error {error:?})");
            return Err(if error == BluetoothError::ResourceInUse {
                HfpError::Blocked
            } else {
                HfpError::NoService
            });
        }
        let service = services.GetAt(0)?;
        let access = service.RequestAccessAsync()?.await?;
        log::info!("hands-free service access: {access:?}");
        if access != DeviceAccessStatus::Allowed {
            return Err(HfpError::Blocked);
        }
        let socket = StreamSocket::new()?;
        let connected = socket
            .ConnectWithProtectionLevelAsync(
                &service.ConnectionHostName()?,
                &service.ConnectionServiceName()?,
                SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
            )?
            .await;
        match connected {
            Ok(()) => {}
            Err(e) if e.code().0 == E_ACCESSDENIED => return Err(HfpError::Blocked),
            Err(e) => return Err(e.into()),
        }
        let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
        Ok(Self {
            socket,
            reader,
            writer,
            buf: String::new(),
        })
    }

    async fn send(&mut self, command: &str) -> Result<(), HfpError> {
        log::debug!("HFP > {}", at::loggable(command));
        self.writer.WriteBytes(command.as_bytes())?;
        self.writer.StoreAsync()?.await?;
        Ok(())
    }

    /// The next complete lines from the phone, waiting up to `wait`.
    async fn lines(&mut self, wait: Duration) -> Result<Vec<String>, HfpError> {
        tokio::time::timeout(wait, async {
            loop {
                let lines = at::take_lines(&mut self.buf);
                if !lines.is_empty() {
                    for l in &lines {
                        log::debug!("HFP < {}", at::loggable(l));
                    }
                    return Ok(lines);
                }
                let got = self.reader.LoadAsync(256)?.await?;
                if got == 0 {
                    return Err(HfpError::Closed);
                }
                let mut chunk = vec![0; got as usize];
                self.reader.ReadBytes(&mut chunk)?;
                self.buf.push_str(&String::from_utf8_lossy(&chunk));
            }
        })
        .await
        .map_err(|_| HfpError::Timeout)?
    }

    fn close(self) {
        let _ = self.socket.Close();
    }
}

/// Connect, set up the hands-free link and, with a number, dial it; then stay `hold` to hear
/// how the call starts before letting go (the call carries on, on the phone).
pub async fn run(device_id: &str, number: Option<&str>, hold: Duration) -> Result<Report, HfpError> {
    // Refuse a bad number before touching the phone.
    let dial = number.map(at::dial).transpose()?;
    let mut link = Link::open(device_id).await?;
    let result = drive(&mut link, dial, hold).await;
    link.close();
    result
}

async fn drive(link: &mut Link, dial: Option<String>, hold: Duration) -> Result<Report, HfpError> {
    let (mut slc, first) = Slc::start();
    link.send(&first).await?;
    while !slc.is_ready() {
        for line in link.lines(REPLY_TIMEOUT).await? {
            if let Next::Send(cmd) = slc.on_reply(&at::parse_reply(&line))? {
                link.send(&cmd).await?;
            }
        }
    }
    log::info!(
        "hands-free link up (phone features {:?}, indicators {:?})",
        slc.ag_features,
        slc.indicators
    );
    let mut report = Report {
        ag_features: slc.ag_features,
        indicators: slc.indicators.clone(),
        ..Report::default()
    };
    let Some(dial) = dial else {
        return Ok(report);
    };
    link.send(&dial).await?;
    while !report.dialed {
        for line in link.lines(REPLY_TIMEOUT).await? {
            let reply = at::parse_reply(&line);
            match reply {
                Reply::Ok => report.dialed = true,
                Reply::Error | Reply::CmeError(_) => return Err(HfpError::DialRefused(line)),
                _ => report.progress.extend(slc.progress(&reply)),
            }
        }
    }
    let until = tokio::time::Instant::now() + hold;
    while let Some(left) = until.checked_duration_since(tokio::time::Instant::now()) {
        match link.lines(left).await {
            Ok(lines) => report
                .progress
                .extend(lines.iter().filter_map(|l| slc.progress(&at::parse_reply(l)))),
            // Quiet, or the phone let go: either way the call is the phone's now.
            Err(HfpError::Timeout | HfpError::Closed) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(report)
}
