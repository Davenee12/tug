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

impl From<windows::core::Error> for BleError {
    fn from(e: windows::core::Error) -> Self {
        Self::Win(e)
    }
}

impl fmt::Display for BleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

/// Register `on_value` for notifications and enable them on the peripheral.
pub async fn subscribe(ch: &GattCharacteristic, on_value: impl Fn(Vec<u8>) + Send + 'static) -> Result<()> {
    ch.ValueChanged(
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
    enable_notify(ch).await
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

pub async fn write(ch: &GattCharacteristic, bytes: &[u8]) -> Result<()> {
    let res = ch
        .WriteValueWithResultAndOptionAsync(&to_buffer(bytes)?, GattWriteOption::WriteWithResponse)?
        .await?;
    check(res.Status()?, res.ProtocolError())
}

pub async fn read(ch: &GattCharacteristic) -> Result<Vec<u8>> {
    let res = ch.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?.await?;
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
