//! Finding and pairing an iPhone: device discovery, custom pairing with the PIN dialog, choosing and forgetting a device.

use super::link::resolve_le_device;
use super::*;

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
            let props = IIterable::<HSTRING>::from(vec![
                HSTRING::from(PROP_IS_CONNECTED),
                HSTRING::from("System.Devices.Aep.IsPaired"),
            ]);
            let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
                &HSTRING::from(aqs),
                &props,
                DeviceInformationKind::AssociationEndpoint,
            )?;
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
                Some(DiscoveredDevice {
                    id: id.clone(),
                    name,
                    transport: d.transport,
                    paired: pairing.as_ref().and_then(|p| p.IsPaired().ok()).unwrap_or(false),
                    can_pair: pairing.as_ref().and_then(|p| p.CanPair().ok()).unwrap_or(false),
                    connected,
                })
            })
            .collect();
        // Connected first: when the iPhone connects from LightBlue it jumps to the top.
        list.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
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
        self.stop_discovery();
        self.shared.update_status(|s| {
            s.device = Some(PairedDevice { id: le_id, name });
            s.connection = ConnectionState::Disconnected;
            s.last_error = None;
        });
        Ok(())
    }

    pub(super) async fn forget(&mut self) -> Result<(), String> {
        let id = self.device_id.take();
        self.drop_link();
        let store = &self.shared.store;
        let _ = store.delete_setting(keys::DEVICE_ID);
        let _ = store.delete_setting(keys::DEVICE_NAME);
        self.shared.update_status(|s| {
            s.device = None;
            s.connection = ConnectionState::NoDevice;
            s.last_error = None;
        });
        if let Some(id) = id {
            // Best effort: a stale Windows bond makes re-pairing fail silently.
            if let Ok(info) = async { DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))?.await }.await {
                if let Ok(op) = info.Pairing().and_then(|p| p.UnpairAsync()) {
                    let _ = op.await;
                }
            }
        }
        Ok(())
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
    custom.PairingRequested(&TypedEventHandler::<
        DeviceInformationCustomPairing,
        DevicePairingRequestedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        log::info!("pairing requested by Windows: kind {:?}", args.PairingKind()?);
        if args.PairingKind()? == DevicePairingKinds::ConfirmOnly {
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
    result.Status()
}

pub(super) fn bool_property(info: &DeviceInformation, key: &str) -> bool {
    info.Properties()
        .and_then(|p| p.Lookup(&HSTRING::from(key)))
        .and_then(|v| v.cast::<IReference<bool>>())
        .and_then(|r| r.Value())
        .unwrap_or(false)
}
