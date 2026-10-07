//! Thin helpers over the WinRT GATT client API.

use std::fmt;
use std::time::Duration;

use windows::core::GUID;
use windows::Devices::Bluetooth::BluetoothCacheMode;
use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
    GattDeviceService, GattValueChangedEventArgs, GattWriteOption,
};
use windows::Foundation::{IReference, TypedEventHandler};
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};

#[derive(Debug)]
pub enum BleError {
    Win(windows::core::Error),
    Unreachable,
    /// Windows didn't finish the operation within its time limit. Unlike `Unreachable` (Windows
    /// answering promptly that the link is down), a run of these on a link that still reports
    /// connected means the adapter itself has stopped responding (see `wedge`).
    TimedOut,
    AccessDenied,
    Protocol(Option<u8>),
    NotFound(&'static str),
}

/// ERROR_BAD_COMMAND: what Windows reports when it connects with a bond the iPhone no
/// longer has (the PC was forgotten on the phone).
const STALE_BOND: windows::core::HRESULT = windows::core::HRESULT(0x8007_0016_u32 as i32);

impl BleError {
    pub fn is_stale_bond(&self) -> bool {
        matches!(self, Self::Win(e) if e.code() == STALE_BOND)
    }

    /// RO_E_CLOSED: Windows tore down tug's GATT objects (e.g. the device's Bluetooth services
    /// were changed in Settings) while the link itself still looks connected. Only a fresh
    /// connection brings them back.
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Win(e) if e.code() == windows::core::HRESULT(0x8000_0013_u32 as i32))
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::TimedOut)
    }

    /// The ATT error code the peripheral answered with, when it answered with one.
    pub fn att_code(&self) -> Option<u8> {
        match self {
            Self::Protocol(code) => *code,
            _ => None,
        }
    }
}

impl From<windows::core::Error> for BleError {
    fn from(e: windows::core::Error) -> Self {
        Self::Win(e)
    }
}

impl fmt::Display for BleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win(_) if self.is_stale_bond() => f.write_str(
                "Your iPhone no longer has this PC paired for notifications (it was forgotten on the phone). Pair again to fix it.",
            ),
            Self::Win(e) => write!(f, "Windows Bluetooth error: {}", e.message()),
            Self::Unreachable => f.write_str("iPhone is out of range or not connected"),
            Self::TimedOut => f.write_str("iPhone is out of range or not responding"),
            Self::AccessDenied => f.write_str("Windows denied access to the device"),
            // ATT: 0x05 insufficient authentication, 0x08 authorization, 0x0F encryption.
            Self::Protocol(Some(0x05 | 0x08 | 0x0F)) => {
                f.write_str("The iPhone requires pairing before it shares notifications")
            }
            Self::Protocol(Some(code)) => write!(f, "iPhone rejected the request (ATT error 0x{code:02X})"),
            Self::Protocol(None) => f.write_str("iPhone rejected the request"),
            Self::NotFound(what) => write!(f, "{what} not found on the iPhone"),
        }
    }
}

pub type Result<T> = std::result::Result<T, BleError>;

/// A full 128-bit UUID from a Bluetooth SIG 16-bit one.
pub fn sig_uuid(short: u16) -> GUID {
    GUID::from_u128(0x0000_0000_0000_1000_8000_0080_5F9B_34FB | ((short as u128) << 96))
}

pub fn to_buffer(bytes: &[u8]) -> windows::core::Result<IBuffer> {
    let w = DataWriter::new()?;
    w.WriteBytes(bytes)?;
    w.DetachBuffer()
}

pub fn from_buffer(buf: &IBuffer) -> windows::core::Result<Vec<u8>> {
    let r = DataReader::FromBuffer(buf)?;
    let mut out = vec![0; r.UnconsumedBufferLength()? as usize];
    r.ReadBytes(&mut out)?;
    Ok(out)
}

fn check(status: GattCommunicationStatus, protocol_error: windows::core::Result<IReference<u8>>) -> Result<()> {
    match status {
        GattCommunicationStatus::Success => Ok(()),
        GattCommunicationStatus::Unreachable => Err(BleError::Unreachable),
        GattCommunicationStatus::AccessDenied => Err(BleError::AccessDenied),
        _ => Err(BleError::Protocol(protocol_error.and_then(|r| r.Value()).ok())),
    }
}

/// First GATT service with `uuid`, going to the device rather than the cache so
/// a fresh link (or a fresh bond) is reflected.
pub async fn service(device: &BluetoothLEDevice, uuid: GUID) -> Result<Option<GattDeviceService>> {
    let device = device.clone();
    off_thread(DISCOVERY_TIMEOUT, move || {
        let res = device
            .GetGattServicesForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)?
            .join()?;
        check(res.Status()?, res.ProtocolError())?;
        let services = res.Services()?;
        Ok(if services.Size()? > 0 {
            Some(services.GetAt(0)?)
        } else {
            None
        })
    })
    .await
}

pub async fn characteristic(service: &GattDeviceService, uuid: GUID, name: &'static str) -> Result<GattCharacteristic> {
    let service = service.clone();
    off_thread(DISCOVERY_TIMEOUT, move || {
        let res = service
            .GetCharacteristicsForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)?
            .join()?;
        check(res.Status()?, res.ProtocolError())?;
        let chars = res.Characteristics()?;
        if chars.Size()? == 0 {
            return Err(BleError::NotFound(name));
        }
        Ok(chars.GetAt(0)?)
    })
    .await
}

/// A live notification subscription. Dropping it unregisters the handler, so a setup
/// that fails halfway, or is retried, can't leave stale handlers stacked on the
/// characteristic delivering every value twice.
#[must_use = "dropping a Subscription unregisters its handler"]
pub struct Subscription {
    ch: GattCharacteristic,
    token: i64,
}

impl Subscription {
    pub fn characteristic(&self) -> &GattCharacteristic {
        &self.ch
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Err(e) = self.ch.RemoveValueChanged(self.token) {
            log::debug!("removing a GATT notification handler failed: {e}");
        }
    }
}

/// Register `on_value` for notifications and enable them on the peripheral. The
/// handler stays registered for as long as the returned `Subscription` lives.
pub async fn subscribe(ch: &GattCharacteristic, on_value: impl Fn(Vec<u8>) + Send + 'static) -> Result<Subscription> {
    let token = ch.ValueChanged(
        &TypedEventHandler::<GattCharacteristic, GattValueChangedEventArgs>::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                match args.CharacteristicValue().and_then(|b| from_buffer(&b)) {
                    Ok(bytes) => on_value(bytes),
                    Err(e) => log::warn!("unreadable GATT notification: {e}"),
                }
            }
            Ok(())
        }),
    )?;
    // Owned from here on: if enabling fails, dropping it removes the handler again.
    let sub = Subscription { ch: ch.clone(), token };
    enable_notify(ch).await?;
    Ok(sub)
}

/// Write the CCCD on the peripheral to turn notifications on.
pub async fn enable_notify(ch: &GattCharacteristic) -> Result<()> {
    let ch = ch.clone();
    // On a fresh bond iOS holds this write open until "Allow" is tapped on the phone.
    off_thread(SUBSCRIBE_TIMEOUT, move || {
        let res = ch
            .WriteClientCharacteristicConfigurationDescriptorWithResultAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )?
            .join()?;
        check(res.Status()?, res.ProtocolError())
    })
    .await
}

/// Whether notifications are currently enabled on the peripheral, read back
/// from the device. The CCCD is shared by every app on this PC using the link.
pub async fn notify_enabled(ch: &GattCharacteristic) -> Result<bool> {
    let ch = ch.clone();
    off_thread(GATT_OP_TIMEOUT, move || {
        let res = ch.ReadClientCharacteristicConfigurationDescriptorAsync()?.join()?;
        check(res.Status()?, res.ProtocolError())?;
        Ok(res.ClientCharacteristicConfigurationDescriptor()?
            == GattClientCharacteristicConfigurationDescriptorValue::Notify)
    })
    .await
}

/// Longest a single GATT read or write may take. Windows normally fails these promptly
/// when the link drops, but a stuck one would park the whole actor (and with it the ANCS
/// timeout, reconnects and the CCCD watchdog), so give up and report it as timed out.
const GATT_OP_TIMEOUT: Duration = Duration::from_secs(10);

/// Opening the device and discovering services. Uncached discovery right after pairing
/// takes well over 10 s on an iPhone; giving up sooner tore the whole link down (texts
/// with it) and retried forever.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Subscribing (CCCD write). After a new pairing iOS asks "Allow notifications?" and only
/// answers this write once it's tapped, so leave a person time to see and tap it.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(90);

/// Any WinRT Bluetooth operation already started on the actor's thread, given up on (as timed out)
/// after `limit`: opening the device and discovery can hang just like reads and writes when the
/// phone drops mid-way. Prefer `off_thread` for GATT operations: this bounds only the wait, not the
/// call that started the operation.
pub async fn bounded_for<T>(
    limit: Duration,
    op: impl std::future::IntoFuture<Output = windows::core::Result<T>>,
) -> Result<T> {
    Ok(tokio::time::timeout(limit, op)
        .await
        .map_err(|_| BleError::TimedOut)??)
}

/// Run one GATT operation — start it, wait for it, read its result — on a short-lived helper
/// thread, and await that here, giving up (as timed out) after `limit`.
///
/// Windows can block the *call that starts* an operation, not just the operation. Seen on hardware
/// (2026-10-06): while the Bluetooth adapter was hung, a CCCD read held the actor's thread ~12 s,
/// past its 10 s limit (a timer can't fire on a blocked thread), freezing everything else on it —
/// tug's own heartbeat included, which then mistook the freeze for the PC sleeping. On a helper, the
/// actor only awaits a channel, so the limit always holds. A helper stuck inside Windows exits once
/// Windows finishes or fails the operation (its own ATT timeout, or the device being closed on a
/// relink); the wedge watch stops tug issuing more while the adapter is stuck.
async fn off_thread<T: Send + 'static>(limit: Duration, op: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("tug-gatt-op".into())
        .spawn(move || {
            // The receiver is gone if the actor already gave up: nothing to report to.
            let _ = tx.send(op());
        })
        .map_err(|e| {
            log::warn!("couldn't start a Bluetooth operation thread: {e}");
            BleError::Unreachable
        })?;
    match tokio::time::timeout(limit, rx).await {
        Ok(Ok(result)) => result,
        // The helper ended without answering (it can only panic, which aborts a release build).
        Ok(Err(_)) => Err(BleError::Unreachable),
        Err(_) => Err(BleError::TimedOut),
    }
}

pub async fn write(ch: &GattCharacteristic, bytes: &[u8]) -> Result<()> {
    let (ch, bytes) = (ch.clone(), bytes.to_vec());
    off_thread(GATT_OP_TIMEOUT, move || {
        let res = ch
            .WriteValueWithResultAndOptionAsync(&to_buffer(&bytes)?, GattWriteOption::WriteWithResponse)?
            .join()?;
        check(res.Status()?, res.ProtocolError())
    })
    .await
}

pub async fn read(ch: &GattCharacteristic) -> Result<Vec<u8>> {
    let ch = ch.clone();
    off_thread(GATT_OP_TIMEOUT, move || {
        let res = ch.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?.join()?;
        check(res.Status()?, res.ProtocolError())?;
        Ok(from_buffer(&res.Value()?)?)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_sig_uuids() {
        assert_eq!(
            format!("{:?}", sig_uuid(0x180F)),
            "0000180F-0000-1000-8000-00805F9B34FB"
        );
    }

    #[test]
    fn a_timeout_reads_as_a_timeout_not_a_refusal() {
        assert!(BleError::TimedOut.is_timeout());
        assert!(!BleError::Unreachable.is_timeout());
        assert!(!BleError::Protocol(Some(0x0E)).is_timeout());
        assert_eq!(BleError::TimedOut.att_code(), None);
    }

    #[test]
    fn an_operation_that_never_finishes_is_given_up_on_and_the_caller_keeps_going() {
        // The 2026-10-06 stall: the operation blocks its thread far past the limit. The awaiting
        // side must still get its timeout on time.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let r: Result<()> = rt.block_on(off_thread(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        }));
        assert!(matches!(r, Err(BleError::TimedOut)));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "not held by the stuck operation"
        );
    }

    #[test]
    fn an_operation_that_finishes_returns_its_result() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let r = rt.block_on(off_thread(Duration::from_secs(5), || Ok(7)));
        assert_eq!(r.ok(), Some(7));
        let r: Result<()> = rt.block_on(off_thread(Duration::from_secs(5), || {
            Err(BleError::Protocol(Some(0xA2)))
        }));
        assert_eq!(r.err().and_then(|e| e.att_code()), Some(0xA2));
    }
}
