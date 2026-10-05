//! Finding and pairing an iPhone: device discovery, custom pairing with the PIN dialog, choosing and forgetting a device.

use super::link::resolve_le_device;
use super::*;
use crate::device_kind;
use crate::state::DeviceKind;

const PROP_LE_APPEARANCE: &str = "System.Devices.Aep.Bluetooth.Le.Appearance";
const PROP_COD_MAJOR: &str = "System.Devices.Aep.Bluetooth.Cod.Major";

impl Actor {
    pub(super) fn start_discovery(&mut self) -> windows::core::Result<()> {
        if !self.watchers.is_empty() {
            self.emit_discovered();
            return Ok(());
        }
        self.discovered.clear();
        let le = format!("System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_LE}\"");
        let classic = format!(
            "System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_CLASSIC}\" AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True"
        );
        for (aqs, transport) in [(le, Transport::Le), (classic, Transport::Classic)] {
            let kind_prop = match transport {
                Transport::Le => PROP_LE_APPEARANCE,
                Transport::Classic => PROP_COD_MAJOR,
            };
            let create = |extra: &[&str]| {
                let mut props = vec![
                    HSTRING::from(PROP_IS_CONNECTED),
                    HSTRING::from("System.Devices.Aep.IsPaired"),
                ];
                props.extend(extra.iter().map(|p| HSTRING::from(*p)));
                DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
                    &HSTRING::from(aqs.as_str()),
                    &IIterable::<HSTRING>::from(props),
                    DeviceInformationKind::AssociationEndpoint,
                )
            };
            // The kind property is a nicety: if this Windows build rejects it, discover without it.
            let watcher = create(&[kind_prop]).or_else(|e| {
                log::warn!("discovery without {kind_prop}: {}", e.message());
                create(&[])
            })?;
            let tx = self.tx.clone();
            watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(
                move |_, info| {
                    if let Some(info) = info.as_ref() {
                        let _ = tx.send(Event::DeviceAdded(info.clone(), transport));
                    }
                    Ok(())
                },
            ))?;
            let tx = self.tx.clone();
            watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
                move |_, u| {
                    if let Some(u) = u.as_ref() {
                        let _ = tx.send(Event::DeviceUpdated(u.clone()));
                    }
                    Ok(())
                },
            ))?;
            let tx = self.tx.clone();
            watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
                move |_, u| {
                    if let Some(id) = u.as_ref().and_then(|u| u.Id().ok()) {
                        let _ = tx.send(Event::DeviceRemoved(id.to_string()));
                    }
                    Ok(())
                },
            ))?;
            watcher.Start()?;
            self.watchers.push(watcher);
        }
        Ok(())
    }

    pub(super) fn stop_discovery(&mut self) {
        for w in self.watchers.drain(..) {
            let _ = w.Stop();
        }
        self.discovered.clear();
    }

    pub(super) fn emit_discovered(&self) {
        let mut list: Vec<DiscoveredDevice> = self
            .discovered
            .iter()
            .filter_map(|(id, d)| {
                let connected = bool_property(&d.info, PROP_IS_CONNECTED);
                let name = d.info.Name().map(|n| n.to_string()).unwrap_or_default();
                // A just-connected iPhone can appear before Windows learns its name.
                let name = match (name.is_empty(), connected) {
                    (false, _) => name,
                    (true, true) => "Unnamed device".to_string(),
                    (true, false) => return None,
                };
                let pairing = d.info.Pairing().ok();
                let kind = device_kind::classify(
                    &name,
                    uint_property(&d.info, PROP_LE_APPEARANCE).and_then(|a| u16::try_from(a).ok()),
                    uint_property(&d.info, PROP_COD_MAJOR),
                );
                Some(DiscoveredDevice {
                    id: id.clone(),
                    name,
                    transport: d.transport,
                    paired: pairing.as_ref().and_then(|p| p.IsPaired().ok()).unwrap_or(false),
                    can_pair: pairing.as_ref().and_then(|p| p.CanPair().ok()).unwrap_or(false),
                    connected,
                    kind,
                })
            })
            .collect();
        // Phones first, accessories last; within that, connected first: when the iPhone connects
        // from LightBlue it jumps to the top.
        let rank = |k: DeviceKind| match k {
            DeviceKind::Phone => 0,
            DeviceKind::Unknown => 1,
            DeviceKind::Accessory => 2,
        };
        list.sort_by(|a, b| {
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then(b.connected.cmp(&a.connected))
                .then(b.paired.cmp(&a.paired))
                .then(a.name.cmp(&b.name))
        });
        self.shared.emit(events::DISCOVERED_DEVICES, list);
    }

    /// For a Classic-paired phone, the connected-but-unpaired LE device that is
    /// almost certainly the same phone: same name, or the only such candidate.
    pub(super) fn le_side_of_classic(&self, classic_id: &str) -> Option<(String, String)> {
        let classic = self.discovered.get(classic_id)?;
        if classic.transport != Transport::Classic {
            return None;
        }
        let name = classic.info.Name().ok()?.to_string();
        let candidates: Vec<(&String, String)> = self
            .discovered
            .iter()
            .filter(|(_, d)| d.transport == Transport::Le && bool_property(&d.info, PROP_IS_CONNECTED))
            .filter(|(_, d)| !d.info.Pairing().and_then(|p| p.IsPaired()).unwrap_or(false))
            .map(|(id, d)| (id, d.info.Name().map(|n| n.to_string()).unwrap_or_default()))
            // The lone-candidate guess below must never land on headphones or a keyboard.
            .filter(|(_, n)| device_kind::classify(n, None, None) != DeviceKind::Accessory)
            .collect();
        let pick = candidates
            .iter()
            .find(|(_, n)| *n == name)
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))?;
        Some((pick.0.clone(), name))
    }

    pub(super) fn pair(&mut self, id: String, reply: Reply) {
        let Some(d) = self.discovered.get(&id) else {
            let _ = reply.send(Err("That device is no longer in range".into()));
            return;
        };
        let info = d.info.clone();
        log::info!(
            "pairing started: {}",
            info.Name().map(|n| n.to_string()).unwrap_or_default()
        );
        let shared = self.shared.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_local(async move {
            let result = pair_device(&info, shared)
                .await
                .map_err(|e| e.message().to_string())
                .and_then(|status| match status {
                    DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
                    DevicePairingResultStatus::RejectedByHandler | DevicePairingResultStatus::PairingCanceled => {
                        Err("Pairing was cancelled".to_string())
                    }
                    DevicePairingResultStatus::AuthenticationTimeout => Err("Pairing timed out".to_string()),
                    other => Err(format!("Pairing failed ({other:?})")),
                });
            let _ = tx.send(Event::PairingDone { id, result, reply });
        });
    }

    /// Adopt a device as "the iPhone": persist it and start connecting.
    pub(super) async fn use_device(&mut self, id: &str) -> Result<(), String> {
        let transport = self.discovered.get(id).map(|d| d.transport);
        let le = resolve_le_device(id, transport).await.map_err(|e| {
            // WinRT reports "no such device" as a null result with S_OK.
            if e.code().is_ok() && transport == Some(Transport::Classic) {
                "Windows has this iPhone paired only for calls and audio (Classic Bluetooth), not Bluetooth LE, which notifications need. In LightBlue on the iPhone, tap the Unnamed entry for this PC, then click Use again."
                    .to_string()
            } else if e.code().is_ok() {
                "Windows couldn't find that device over Bluetooth LE. Reconnect from LightBlue and try again.".to_string()
            } else {
                format!("Couldn't open that device over Bluetooth LE: {}", e.message())
            }
        })?;
        let le_id = le.DeviceId().map_err(|e| e.message().to_string())?.to_string();
        log::info!("using device {le_id}");
        let name = le.Name().map(|n| n.to_string()).unwrap_or_default();
        let carried = self.carried_name.take();
        let name = match (name.is_empty(), carried) {
            (false, _) => name,
            (true, Some(c)) => c,
            (true, None) => "iPhone".to_string(),
        };
        let store = &self.shared.store;
        store.set_setting(keys::DEVICE_ID, &le_id).map_err(|e| e.to_string())?;
        store.set_setting(keys::DEVICE_NAME, &name).map_err(|e| e.to_string())?;
        self.drop_link();
        self.device_id = Some(le_id.clone());
        self.retry_in = 0;
        self.connect_failures = 0;
        self.stop_discovery();
        self.shared.update_status(|s| {
            s.device = Some(PairedDevice { id: le_id, name });
            s.connection = ConnectionState::Disconnected;
            s.last_error = None;
            s.pairing_stale = false;
        });
        Ok(())
    }

    /// Start over: drop and unpair both bonds tug knows about — the notifications (LE) device
    /// and the remembered texts (Classic) device — and clear the remembered ids, so a phone the
    /// user replaced (or that forgot this PC) leaves nothing stale behind for the next pairing.
    pub(super) async fn forget(&mut self) -> Result<(), String> {
        let le_id = self.device_id.take();
        // Read the texts device id before deleting the setting, so we can unpair it too.
        let texts_id = self.shared.store.setting(keys::TEXTS_DEVICE_ID).ok().flatten();
        self.drop_link();
        let store = &self.shared.store;
        let _ = store.delete_setting(keys::DEVICE_ID);
        let _ = store.delete_setting(keys::DEVICE_NAME);
        // Forget the remembered texts phone too, so a new phone isn't matched to the old id.
        let _ = store.delete_setting(keys::TEXTS_DEVICE_ID);
        self.shared.update_status(|s| {
            s.device = None;
            s.connection = ConnectionState::NoDevice;
            s.last_error = None;
            s.pairing_stale = false;
            s.awaiting_phone_allow = false;
            s.texts_pairing = crate::map::health::TextsPairing::Unknown;
            s.texts_device = None;
            s.messages_error = None;
            s.contacts_error = None;
            s.services = Services::default();
        });
        // Best effort: a stale Windows bond makes re-pairing fail silently. The Classic (texts)
        // bond and the LE (notifications) bond are separate Windows pairings; remove both.
        for id in [le_id, texts_id].into_iter().flatten() {
            unpair_device(&id).await;
        }
        Ok(())
    }

    /// Pair the iPhone's Classic (texts) side from inside tug, so the Texts step doesn't have to
    /// send people to Windows › Add device. LE (notifications) must be paired first — cross-
    /// transport key derivation depends on that order. Runs a one-shot inquiry for unpaired
    /// Classic devices (the iPhone must have Settings › Bluetooth open), picks the one matching
    /// the adopted phone with pure logic, pairs it with the PIN shown in tug, and remembers it.
    pub(super) async fn pair_texts(&mut self) -> Result<(), String> {
        let le_name = match self.shared.status().device {
            Some(d) => d.name,
            None => return Err("Pair your iPhone for notifications first, then set up texts.".into()),
        };
        let candidates = discover_unpaired_classic().await?;
        let pick = crate::map::pick::choose_texts_candidate(&candidates, &le_name).ok_or_else(|| {
            "Couldn't find your iPhone to pair for texts. On the iPhone, open Settings › Bluetooth and keep it on screen, then try again."
                .to_string()
        })?;
        let id = pick.id.clone();
        log::info!("pairing for texts: {} ({id})", pick.name);
        let op =
            DeviceInformation::CreateFromIdAsync(&HSTRING::from(id.as_str())).map_err(|e| e.message().to_string())?;
        let info = winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, op)
            .await
            .map_err(|e| e.to_string())?;
        let status = pair_device(&info, self.shared.clone())
            .await
            .map_err(|e| e.message().to_string())?;
        match status {
            DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => {}
            DevicePairingResultStatus::RejectedByHandler | DevicePairingResultStatus::PairingCanceled => {
                return Err("Texts pairing was cancelled".into())
            }
            DevicePairingResultStatus::AuthenticationTimeout => return Err("Texts pairing timed out".into()),
            other => return Err(format!("Texts pairing failed ({other:?})")),
        }
        // Remember this phone as the texts device and ask the message service to connect now.
        let _ = self.shared.store.set_setting(keys::TEXTS_DEVICE_ID, &id);
        if let Some(map) = self.shared.map.get() {
            map.refresh();
        }
        log::info!("paired for texts; message service will connect");
        Ok(())
    }
}

/// One-shot inquiry for unpaired Classic devices (`GetDeviceSelectorFromPairingState(false)`).
/// Only run during the Texts step; the iPhone appears only while its Settings › Bluetooth screen
/// is open. Bounded, so a quiet radio can't park the actor.
async fn discover_unpaired_classic() -> std::result::Result<Vec<crate::map::pick::UnpairedDevice>, String> {
    use crate::map::pick::UnpairedDevice;
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(false).map_err(|e| e.message().to_string())?;
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(PROP_COD_MAJOR)]);
    let op = DeviceInformation::FindAllAsyncAqsFilterAndAdditionalProperties(&selector, &props)
        .map_err(|e| e.message().to_string())?;
    let infos = winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, op)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for info in infos {
        let Ok(id) = info.Id() else { continue };
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        let kind = device_kind::classify(&name, None, uint_property(&info, PROP_COD_MAJOR));
        log::info!("texts inquiry: {name:?} ({kind:?})");
        out.push(UnpairedDevice {
            id: id.to_string(),
            name,
            kind,
        });
    }
    Ok(out)
}

/// Remove one Windows Bluetooth bond by device id. Best effort and fully bounded, so a phone
/// that's away (CreateFromIdAsync or UnpairAsync hanging) can't park the actor loop.
pub(super) async fn unpair_device(id: &str) {
    let op = match DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)) {
        Ok(op) => op,
        Err(e) => return log::debug!("unpair {id}: {}", e.message()),
    };
    let info = match winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, op).await {
        Ok(info) => info,
        Err(e) => return log::debug!("unpair {id}: {e}"),
    };
    if let Ok(op) = info.Pairing().and_then(|p| p.UnpairAsync()) {
        match winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, op).await {
            Ok(status) => log::info!("unpaired {id}: {:?}", status.Status()),
            Err(e) => log::debug!("unpair {id}: {e}"),
        }
    }
}

/// Pair with a PIN prompt routed through the tug UI.
pub(super) async fn pair_device(
    info: &DeviceInformation,
    shared: Arc<Shared>,
) -> windows::core::Result<DevicePairingResultStatus> {
    let pairing = info.Pairing()?;
    if pairing.IsPaired()? {
        return Ok(DevicePairingResultStatus::AlreadyPaired);
    }
    let device_name = Some(info.Name()?.to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "your iPhone".to_string());
    let custom = pairing.Custom()?;
    // Cleared after pairing resolves, so a ConfirmOnly prompt (which Windows accepts on its own,
    // with no deferral thread to clear it) doesn't linger in tug.
    let shared_for_close = shared.clone();
    custom.PairingRequested(&TypedEventHandler::<
        DeviceInformationCustomPairing,
        DevicePairingRequestedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        log::info!("pairing requested by Windows: kind {:?}", args.PairingKind()?);
        if args.PairingKind()? == DevicePairingKinds::ConfirmOnly {
            // Windows accepts on its own; the user taps "Pair" on the iPhone. Show that in tug so
            // the screen isn't blank while the phone waits for them.
            shared.emit(
                events::PAIRING_REQUEST,
                PairingRequest {
                    device_name: device_name.clone(),
                    pin: None,
                    confirm_on_phone: true,
                },
            );
            return args.Accept();
        }
        // ConfirmPinMatch / DisplayPin: show the code and let the user decide.
        let pin = args.Pin().ok().map(|p| p.to_string()).filter(|p| !p.is_empty());
        let deferral = args.GetDeferral()?;
        let (tx, rx) = sync_channel(1);
        shared.set_pairing_confirm(Some(tx));
        shared.emit(
            events::PAIRING_REQUEST,
            PairingRequest {
                device_name: device_name.clone(),
                pin,
                confirm_on_phone: false,
            },
        );
        let args = args.clone();
        let shared = shared.clone();
        std::thread::spawn(move || {
            if rx.recv_timeout(PIN_CONFIRM_TIMEOUT).unwrap_or(false) {
                let _ = args.Accept();
            }
            let _ = deferral.Complete();
            shared.set_pairing_confirm(None);
            shared.emit(events::PAIRING_REQUEST_CLOSED, ());
        });
        Ok(())
    }))?;
    let kinds = DevicePairingKinds::ConfirmOnly | DevicePairingKinds::ConfirmPinMatch | DevicePairingKinds::DisplayPin;
    let result = custom
        .PairWithProtectionLevelAsync(kinds, DevicePairingProtectionLevel::Encryption)?
        .await?;
    // Clear any informational prompt (ConfirmOnly has no deferral thread to do it).
    shared_for_close.emit(events::PAIRING_REQUEST_CLOSED, ());
    result.Status()
}

pub(super) fn bool_property(info: &DeviceInformation, key: &str) -> bool {
    info.Properties()
        .and_then(|p| p.Lookup(&HSTRING::from(key)))
        .and_then(|v| v.cast::<IReference<bool>>())
        .and_then(|r| r.Value())
        .unwrap_or(false)
}

/// An unsigned-integer property, whichever width Windows stored it as.
fn uint_property(info: &DeviceInformation, key: &str) -> Option<u32> {
    let v = info.Properties().and_then(|p| p.Lookup(&HSTRING::from(key))).ok()?;
    if let Ok(r) = v.cast::<IReference<u16>>() {
        return r.Value().ok().map(u32::from);
    }
    if let Ok(r) = v.cast::<IReference<u8>>() {
        return r.Value().ok().map(u32::from);
    }
    v.cast::<IReference<u32>>().and_then(|r| r.Value()).ok()
}
