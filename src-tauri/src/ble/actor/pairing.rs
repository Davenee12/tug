//! Finding and pairing an iPhone: device discovery, custom pairing with the PIN dialog, choosing and forgetting a device.

use super::link::resolve_le_device;
use super::*;
use crate::device_kind;
use crate::state::DeviceKind;

pub(super) const PROP_LE_APPEARANCE: &str = "System.Devices.Aep.Bluetooth.Le.Appearance";
pub(super) const PROP_COD_MAJOR: &str = "System.Devices.Aep.Bluetooth.Cod.Major";

impl Actor {
    pub(super) fn start_discovery(&mut self) -> windows::core::Result<()> {
        if !self.watchers.is_empty() {
            self.emit_discovered();
            return Ok(());
        }
        self.discovered.clear();
        for transport in [Transport::Le, Transport::Classic] {
            let watcher = self.build_watcher(transport)?;
            watcher.Start()?;
            self.watchers.push((transport, watcher));
        }
        Ok(())
    }

    /// Build (but don't start) a device watcher for one transport, wired to feed the actor's event
    /// channel. Shared by `start_discovery` and `rescan_classic`.
    ///
    /// No IsPaired filter on Classic: a freshly-forgotten iPhone sitting on Settings › Bluetooth is
    /// discoverable over Classic inquiry with its real name (CoD major = phone), but unpaired.
    /// Filtering to paired-only hid it, so the wizard only ever saw already-paired accessories
    /// (AirPods, Echo) and the anonymous LE adverts. Let both paired and unpaired Classic devices
    /// through; device_kind keeps non-phones out of the phone offer.
    fn build_watcher(&self, transport: Transport) -> windows::core::Result<DeviceWatcher> {
        let (aqs, kind_prop) = match transport {
            Transport::Le => (
                format!("System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_LE}\""),
                PROP_LE_APPEARANCE,
            ),
            Transport::Classic => (
                format!("System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_CLASSIC}\""),
                PROP_COD_MAJOR,
            ),
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
        Ok(watcher)
    }

    /// Restart just the Classic watcher so Windows runs a fresh inquiry: an unpaired-Classic AEP
    /// watcher inquires once when it starts and never again, so an iPhone made discoverable after
    /// scanning began would otherwise never appear. The LE watcher and the rows already discovered
    /// are left untouched (ids stay stable; re-added devices update in place), so nothing flickers.
    pub(super) fn rescan_classic(&mut self) -> windows::core::Result<()> {
        // Nothing to re-inquire if discovery isn't running (the find step drives this).
        if self.watchers.is_empty() {
            return Ok(());
        }
        self.watchers.retain(|(t, w)| {
            if *t == Transport::Classic {
                let _ = w.Stop();
                false
            } else {
                true
            }
        });
        log::debug!("rescan: restarting Classic discovery for the iPhone");
        let watcher = self.build_watcher(Transport::Classic)?;
        watcher.Start()?;
        self.watchers.push((Transport::Classic, watcher));
        Ok(())
    }

    pub(super) fn stop_discovery(&mut self) {
        for (_, w) in self.watchers.drain(..) {
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
        let transport = d.transport;
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        log::info!("pairing started: {name:?} ({transport:?})");
        let shared = self.shared.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_local(async move {
            // Pairing an unpaired Classic iPhone: note which LE devices are already bonded, so once
            // cross-transport key derivation adds the phone's LE bond we can tell it from the rest.
            let before = if transport == Transport::Classic {
                Some(paired_le_ids().await.unwrap_or_else(|e| {
                    log::warn!("couldn't list bonded LE devices before pairing: {e}");
                    std::collections::HashSet::new()
                }))
            } else {
                None
            };
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
            // A Classic iPhone that paired: wait here (off the actor loop, up to ~15 s) for the LE
            // bond cross-transport derivation creates, so adoption is instant and the loop responsive.
            let classic = match (&result, before) {
                (Ok(()), Some(before)) => {
                    let resolved_le = resolve_le_after_classic(&name, &before).await;
                    Some(ClassicPairing { name, resolved_le })
                }
                _ => None,
            };
            let _ = tx.send(Event::PairingDone {
                id,
                result,
                classic,
                reply,
            });
        });
    }

    /// Adopt an iPhone whose Classic side tug just paired. Windows derives a Bluetooth LE bond for
    /// the same phone (cross-transport key derivation); find it, adopt it for notifications, and
    /// remember the Classic device for texts. The LE bond can appear a moment after the Classic
    /// one, so look a few times over a few seconds before falling back to the address mapping.
    pub(super) async fn adopt_le_after_classic(
        &mut self,
        classic_id: String,
        ctx: ClassicPairing,
    ) -> Result<(), String> {
        // Remember the Classic side for texts regardless of how the LE side resolves (or doesn't):
        // the texts pairing is already done, and the message service can connect on it.
        let _ = self.shared.store.set_setting(keys::TEXTS_DEVICE_ID, &classic_id);

        let le_id = match ctx.resolved_le {
            Some(id) => {
                log::info!("LE device resolved after Classic pairing");
                id
            }
            None => {
                // Fall back to the old address mapping: works only when both transports share one
                // address, which an iPhone usually doesn't, but costs nothing to try.
                match resolve_le_device(&classic_id, Some(Transport::Classic)).await {
                    Ok(le) => match le.DeviceId() {
                        Ok(id) => {
                            log::info!("LE device resolved by address mapping after Classic pairing");
                            id.to_string()
                        }
                        Err(e) => return Err(e.message().to_string()),
                    },
                    Err(_) => {
                        log::warn!("no LE device appeared after Classic pairing");
                        return Err(
                            "Almost there — keep Settings › Bluetooth open on your iPhone and tap Pair again."
                                .to_string(),
                        );
                    }
                }
            }
        };
        self.carried_name = Some(ctx.name);
        self.use_device(&le_id).await
    }

    /// Remove a leftover Windows pairing: unpair both the LE and Classic bonds of the phone the id
    /// points at (matched by name, since the two bonds share one), then re-inquire so it reappears
    /// unpaired and ready to pair fresh. Best effort, fully bounded — a bond that's away can't park
    /// the actor loop.
    pub(super) async fn remove_pairing(&mut self, id: String) -> Result<(), String> {
        // The two bonds share a name; collect every paired phone entry with it. Gather the ids
        // first, since the map can't stay borrowed across the await.
        let target_name = self.discovered.get(&id).and_then(|d| real_name(&d.info));
        let mut ids: Vec<String> = self
            .discovered
            .iter()
            .filter(|(_, d)| {
                let name = d.info.Name().map(|n| n.to_string()).unwrap_or_default();
                let kind = device_kind::classify(
                    &name,
                    uint_property(&d.info, PROP_LE_APPEARANCE).and_then(|a| u16::try_from(a).ok()),
                    uint_property(&d.info, PROP_COD_MAJOR),
                );
                let paired = d.info.Pairing().and_then(|p| p.IsPaired()).unwrap_or(false);
                paired
                    && kind == DeviceKind::Phone
                    && target_name
                        .as_deref()
                        .is_some_and(|n| real_name(&d.info).as_deref() == Some(n))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if !ids.contains(&id) {
            ids.push(id);
        }
        log::debug!("removing leftover pairing: {} bond(s)", ids.len());
        for bond in ids {
            unpair_device(&bond).await;
        }
        // Keep scanning so the now-unpaired phone shows up fresh, ready to pair.
        let _ = self.rescan_classic();
        Ok(())
    }

    /// Adopt a device as "the iPhone": persist it and start connecting.
    pub(super) async fn use_device(&mut self, id: &str) -> Result<(), String> {
        let transport = self.discovered.get(id).map(|d| d.transport);
        let le = resolve_le_device(id, transport).await.map_err(|e| {
            // WinRT reports "no such device" as a null result with S_OK.
            if e.code().is_ok() && transport == Some(Transport::Classic) {
                "Windows has this iPhone paired for calls and audio only, not the connection your notifications need. On your iPhone, keep Settings › Bluetooth open and tap Pair again."
                    .to_string()
            } else if e.code().is_ok() {
                "Windows couldn't reach that iPhone over Bluetooth. On your iPhone, keep Settings › Bluetooth open and tap Pair again.".to_string()
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

/// The paired (bonded) Bluetooth LE devices Windows knows, as candidates for the LE side of a
/// just-paired Classic iPhone. Enumeration only (no inquiry), so it's quick; bounded all the same.
async fn paired_le_candidates() -> std::result::Result<Vec<crate::map::pick::LeCandidate>, String> {
    use crate::map::pick::LeCandidate;
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).map_err(|e| e.message().to_string())?;
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(PROP_LE_APPEARANCE)]);
    let op = DeviceInformation::FindAllAsyncAqsFilterAndAdditionalProperties(&selector, &props)
        .map_err(|e| e.message().to_string())?;
    let infos = winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, op)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for info in infos {
        let Ok(id) = info.Id() else { continue };
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        let appearance = uint_property(&info, PROP_LE_APPEARANCE).and_then(|a| u16::try_from(a).ok());
        out.push(LeCandidate {
            id: id.to_string(),
            name: name.clone(),
            kind: device_kind::classify(&name, appearance, None),
        });
    }
    Ok(out)
}

/// Just the ids of the bonded LE devices, to snapshot before a Classic pairing.
async fn paired_le_ids() -> std::result::Result<std::collections::HashSet<String>, String> {
    Ok(paired_le_candidates().await?.into_iter().map(|c| c.id).collect())
}

/// Look for the LE bond cross-transport derivation creates for a just-paired Classic iPhone,
/// retrying for ~15 s because it can show up a little later (one run took three attempts, and one
/// had none appear within the old ~5 s). Runs off the actor loop (in the pairing task), so the wait
/// never blocks other commands. Pure choice in `map::pick`.
async fn resolve_le_after_classic(classic_name: &str, before: &std::collections::HashSet<String>) -> Option<String> {
    const ATTEMPTS: u32 = 15;
    for attempt in 1..=ATTEMPTS {
        match paired_le_candidates().await {
            Ok(candidates) => {
                if let Some(c) = crate::map::pick::choose_le_after_classic(&candidates, classic_name, before) {
                    return Some(c.id.clone());
                }
                log::debug!("LE bond not visible yet after Classic pairing (attempt {attempt}/{ATTEMPTS})");
            }
            Err(e) => log::warn!("listing bonded LE devices after Classic pairing: {e}"),
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    None
}

/// Lower-cased, trimmed device name, or None when it isn't a real one yet ("Unnamed device").
fn real_name(info: &DeviceInformation) -> Option<String> {
    let n = info
        .Name()
        .map(|n| n.to_string())
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    (!n.is_empty() && n != "unnamed device").then_some(n)
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
pub(super) fn uint_property(info: &DeviceInformation, key: &str) -> Option<u32> {
    let v = info.Properties().and_then(|p| p.Lookup(&HSTRING::from(key))).ok()?;
    if let Ok(r) = v.cast::<IReference<u16>>() {
        return r.Value().ok().map(u32::from);
    }
    if let Ok(r) = v.cast::<IReference<u8>>() {
        return r.Value().ok().map(u32::from);
    }
    v.cast::<IReference<u32>>().and_then(|r| r.Value()).ok()
}
