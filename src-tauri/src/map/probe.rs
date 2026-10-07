//! The Classic half of the Bluetooth inventory (`crate::bt_inventory`): the iPhone's SDP records,
//! which PBAP phonebooks answer (asked with MaxListCount = 0, so only sizes come back — no vCards
//! are pulled here; the vCard field names come from tug's own contacts pulls), and the MAP folder
//! names. Thin WinRT/OBEX I/O; the decoding and every privacy rule live in the pure
//! `bt_inventory` module.

use std::time::Duration;

use windows::core::HSTRING;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice};

use super::obex::{self, Header};
use super::session::{MapError, MapSession, ObexLink, PBAP_TARGET, PBAP_TURN, PSE_UUID};
use crate::ble::winrt::{bounded_for, from_buffer};
use crate::bt_inventory::{self as inv, sdp, PbapReport, PhonebookAnswer, SdpRecord};

const SDP_TIMEOUT: Duration = Duration::from_secs(20);
/// Each phonebook size question. fav/spd are PBAP 1.2 extras an iPhone may not answer at all.
const PHONEBOOK_TIMEOUT: Duration = Duration::from_secs(5);
const FOLDERS_TIMEOUT: Duration = Duration::from_secs(15);

const PHONEBOOKS: [&str; 7] = ["pb", "ich", "och", "mch", "cch", "fav", "spd"];

// PBAP application parameters.
const PB_MAX_LIST_COUNT: u8 = 0x04;
const PB_PHONEBOOK_SIZE: u8 = 0x08;
/// A size answer that keeps streaming is the phone ignoring MaxListCount = 0; stop listening.
const MAX_SIZE_PACKETS: usize = 4;

/// Why the inventory's phonebook check didn't produce a report.
#[derive(Debug)]
pub enum PbapProbeError {
    /// tug's own contacts, calls or photo pull holds the phone's one PBAP connection: skipped, so
    /// the check never opens a second one (and never makes tug's pulls wait).
    Busy,
    Failed(String),
}

/// The iPhone's Classic SDP records, described. An uncached RFCOMM query first makes Windows
/// run SDP again; records come from `BluetoothDevice.SdpRecords`, or (if Windows lists none) from
/// each RFCOMM service's raw attributes.
pub async fn sdp_records(device_id: &str) -> Result<Vec<SdpRecord>, String> {
    let device = bounded_for(
        SDP_TIMEOUT,
        BluetoothDevice::FromIdAsync(&HSTRING::from(device_id)).map_err(s)?,
    )
    .await
    .map_err(s)?;
    let rfcomm = bounded_for(
        SDP_TIMEOUT,
        device
            .GetRfcommServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
            .map_err(s)?,
    )
    .await;
    let mut out = Vec::new();
    for buf in device.SdpRecords().map_err(s)? {
        let bytes = from_buffer(&buf).map_err(s)?;
        out.push(describe(sdp::parse_record(&bytes)));
    }
    if out.is_empty() {
        let services = rfcomm.map_err(s)?.Services().map_err(s)?;
        for svc in services {
            let map = bounded_for(
                SDP_TIMEOUT,
                svc.GetSdpRawAttributesWithCacheModeAsync(BluetoothCacheMode::Cached)
                    .map_err(s)?,
            )
            .await
            .map_err(s)?;
            let mut attrs = Vec::new();
            let mut failed = None;
            for pair in map {
                let id = pair.Key().map_err(s)?;
                let raw = from_buffer(&pair.Value().map_err(s)?).map_err(s)?;
                match (u16::try_from(id), sdp::parse_element(&raw)) {
                    (Ok(id), Ok((el, _))) => attrs.push((id, el)),
                    (_, Err(e)) => failed = Some(e),
                    (Err(_), _) => failed = Some(format!("attribute id 0x{id:X} out of range")),
                }
            }
            attrs.sort_by_key(|(id, _)| *id);
            let mut r = sdp::describe_record(&attrs);
            if let Some(e) = failed {
                r.details.insert("parseError".into(), e);
            }
            out.push(r);
        }
    }
    if out.is_empty() {
        return Err("Windows has no SDP records for the iPhone".into());
    }
    Ok(out)
}

fn describe(parsed: Result<Vec<(u16, sdp::Element)>, String>) -> SdpRecord {
    match parsed {
        Ok(attrs) => sdp::describe_record(&attrs),
        Err(e) => {
            let mut r = SdpRecord::default();
            r.details.insert("parseError".into(), e);
            r
        }
    }
}

fn s(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Ask each phonebook for its size (MaxListCount = 0: no vCards are transferred). Holds
/// [`PBAP_TURN`] throughout, and skips at once if tug's own PBAP pull has it. Stops after
/// [`inv::MAX_FAILED_PHONEBOOKS`] failures. Nothing personal is kept: sizes and response codes.
pub async fn pbap_probe(device_id: &str, now_ms: i64) -> Result<PbapReport, PbapProbeError> {
    let Ok(_turn) = PBAP_TURN.try_lock() else {
        return Err(PbapProbeError::Busy);
    };
    let connect = || ObexLink::connect(device_id, PSE_UUID, &PBAP_TARGET, MapError::ContactsConsent);
    let mut link = Some(connect().await.map_err(|e| PbapProbeError::Failed(e.to_string()))?);
    let mut report = PbapReport {
        probed_at: Some(now_ms),
        ..Default::default()
    };
    let mut failed = 0;
    let mut stopped: Option<String> = None;
    for book in PHONEBOOKS {
        let Some(l) = link.as_mut().filter(|_| stopped.is_none()) else {
            let why = stopped.as_deref().unwrap_or("the connection closed");
            report.phonebooks.insert(
                book.to_string(),
                PhonebookAnswer {
                    response: format!("not asked: {why}"),
                    size: None,
                },
            );
            continue;
        };
        let (answer, link_ok) = match tokio::time::timeout(PHONEBOOK_TIMEOUT, phonebook_size(l, book)).await {
            Ok(r) => r,
            Err(_) => (
                PhonebookAnswer {
                    response: "timeout".into(),
                    size: None,
                },
                false,
            ),
        };
        report.phonebooks.insert(book.to_string(), answer);
        if link_ok {
            continue;
        }
        failed += 1;
        // A timed-out or streaming answer may still be arriving: this session is out of step.
        if let Some(l) = link.take() {
            l.disconnect().await;
        }
        if !inv::keep_asking_phonebooks(failed) {
            stopped = Some(format!("{failed} phonebooks failed"));
            continue;
        }
        match connect().await {
            Ok(l) => link = Some(l),
            Err(e) => stopped = Some(format!("couldn't reconnect: {}", inv::safe_text(&e.to_string(), 80))),
        }
    }
    if let Some(l) = link {
        l.disconnect().await;
    }
    Ok(report)
}

/// PullPhoneBook with MaxListCount = 0: the phone answers with PhonebookSize and no vCards.
/// Returns the answer and whether the session is still in step.
async fn phonebook_size(link: &mut ObexLink, book: &str) -> (PhonebookAnswer, bool) {
    let headers = [
        link.conn(),
        Header::type_("x-bt/phonebook"),
        Header::Name(Some(format!("telecom/{book}.vcf"))),
        obex::app_params(&[(PB_MAX_LIST_COUNT, &0u16.to_be_bytes())]),
    ];
    let mut packet = obex::request(obex::OP_GET_FINAL, &[], &headers);
    let mut size = None;
    for _ in 0..MAX_SIZE_PACKETS {
        let resp = match link.exchange(&packet, false).await {
            Ok(r) => r,
            Err(e) => {
                let response = match e {
                    MapError::Timeout => "timeout".to_string(),
                    other => inv::safe_text(&other.to_string(), 80),
                };
                return (PhonebookAnswer { response, size }, false);
            }
        };
        for h in &resp.headers {
            if let Header::Bytes(obex::HI_APP_PARAMS, bytes) = h {
                for (tag, value) in obex::decode_app_params(bytes).unwrap_or_default() {
                    if tag == PB_PHONEBOOK_SIZE && value.len() == 2 {
                        size = Some(u16::from_be_bytes([value[0], value[1]]));
                    }
                }
            }
        }
        if resp.code != obex::RSP_CONTINUE {
            let response = inv::obex_response_name(resp.code);
            return (PhonebookAnswer { response, size }, true);
        }
        packet = obex::request(obex::OP_GET_FINAL, &[], &[link.conn()]);
    }
    (
        PhonebookAnswer {
            response: "kept sending vCards (ignored MaxListCount = 0)".into(),
            size,
        },
        false,
    )
}

/// The MAP folder names under `telecom/msg` (GetFolderListing). Names are cleaned and redacted.
pub async fn folders(session: &mut MapSession) -> Result<Vec<String>, MapError> {
    tokio::time::timeout(FOLDERS_TIMEOUT, async {
        let link = &mut session.link;
        link.set_path(None).await?;
        link.set_path(Some("telecom")).await?;
        link.set_path(Some("msg")).await?;
        let headers = vec![link.conn(), Header::type_("x-obex/folder-listing")];
        let xml = link.get("GetFolderListing", headers).await?;
        Ok(inv::tally::folder_names(&String::from_utf8_lossy(&xml)))
    })
    .await
    .map_err(|_| MapError::Timeout)?
}
