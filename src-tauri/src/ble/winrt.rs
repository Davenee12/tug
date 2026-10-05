//! Thin helpers over the WinRT GATT client API.

use std::fmt;

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
    let res = device
        .GetGattServicesForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)?
        .await?;
    check(res.Status()?, res.ProtocolError())?;
    let services = res.Services()?;
    Ok(if services.Size()? > 0 {
        Some(services.GetAt(0)?)
    } else {
        None
    })
}

pub async fn characteristic(service: &GattDeviceService, uuid: GUID, name: &'static str) -> Result<GattCharacteristic> {
    let res = service
        .GetCharacteristicsForUuidWithCacheModeAsync(uuid, BluetoothCacheMode::Uncached)?
        .await?;
    check(res.Status()?, res.ProtocolError())?;
    let chars = res.Characteristics()?;
    if chars.Size()? == 0 {
        return Err(BleError::NotFound(name));
    }
    Ok(chars.GetAt(0)?)
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
    let res = ch
        .WriteClientCharacteristicConfigurationDescriptorWithResultAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )?
        .await?;
    check(res.Status()?, res.ProtocolError())
}

/// Whether notifications are currently enabled on the peripheral, read back
/// from the device. The CCCD is shared by every app on this PC using the link.
pub async fn notify_enabled(ch: &GattCharacteristic) -> Result<bool> {
    let res = ch.ReadClientCharacteristicConfigurationDescriptorAsync()?.await?;
    check(res.Status()?, res.ProtocolError())?;
    Ok(res.ClientCharacteristicConfigurationDescriptor()?
        == GattClientCharacteristicConfigurationDescriptorValue::Notify)
}

/// Longest a single GATT read or write may take. Windows normally fails these promptly
/// when the link drops, but a stuck one would park the whole actor (and with it the ANCS
/// timeout, reconnects and the CCCD watchdog), so give up and treat it as unreachable.
const GATT_OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn write(ch: &GattCharacteristic, bytes: &[u8]) -> Result<()> {
    let op = ch.WriteValueWithResultAndOptionAsync(&to_buffer(bytes)?, GattWriteOption::WriteWithResponse)?;
    let res = tokio::time::timeout(GATT_OP_TIMEOUT, op)
        .await
        .map_err(|_| BleError::Unreachable)??;
    check(res.Status()?, res.ProtocolError())
}

pub async fn read(ch: &GattCharacteristic) -> Result<Vec<u8>> {
    let op = ch.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?;
    let res = tokio::time::timeout(GATT_OP_TIMEOUT, op)
        .await
        .map_err(|_| BleError::Unreachable)??;
    check(res.Status()?, res.ProtocolError())?;
    Ok(from_buffer(&res.Value()?)?)
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
}
