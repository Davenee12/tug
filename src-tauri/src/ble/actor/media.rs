//! AMS media controls and the Battery Service.

use super::*;

impl Actor {
    pub(super) async fn setup_media(&self, device: &BluetoothLEDevice, gen: u64) -> Result<Media, BleError> {
        let svc = winrt::service(device, guid(ams::SERVICE))
            .await?
            .ok_or(BleError::NotFound("Media service (AMS)"))?;
        let remote = winrt::characteristic(&svc, guid(ams::REMOTE_COMMAND), "AMS remote command").await?;
        let entity = winrt::characteristic(&svc, guid(ams::ENTITY_UPDATE), "AMS entity update").await?;
        // If anything below fails, these drop and their handlers go with them, so the
        // periodic retry can't stack duplicates.
        let tx = self.tx.clone();
        let remote_updates = winrt::subscribe(&remote, move |data| {
            let _ = tx.send(Event::MediaCommands { gen, data });
        })
        .await?;
        let tx = self.tx.clone();
        let entity_updates = winrt::subscribe(&entity, move |data| {
            let _ = tx.send(Event::MediaEntity { gen, data });
        })
        .await?;
        // One entity per write (the spec's rule). The queue only feeds the loop button, so
        // a refused queue registration is logged and media carries on without it.
        for registration in ams::registrations() {
            let name = ams::entity_name(registration[0]);
            match winrt::write(&entity, &registration).await {
                Ok(()) => log::debug!("AMS registered for {name} attributes {:?}", &registration[1..]),
                Err(e) => {
                    log::warn!(
                        "AMS registration for {name} {registration:?} failed: {}",
                        describe_error(&e)
                    );
                    if !ams::registration_is_optional(&registration) {
                        return Err(e);
                    }
                }
            }
        }
        let attribute = match winrt::characteristic(&svc, guid(ams::ENTITY_ATTRIBUTE), "AMS entity attribute").await {
            Ok(c) => Some(c),
            Err(e) => {
                log::info!("AMS current values unavailable: {e}");
                None
            }
        };
        if let Some(attribute) = &attribute {
            self.read_current_media(attribute).await;
        }
        Ok(Media {
            _service: svc,
            remote_command: remote,
            entity_attribute: attribute,
            _remote_updates: remote_updates,
            _entity_update: entity_updates,
        })
    }

    /// Pick up whatever is already playing. The spec says registering sends the current
    /// values too, but reading them is cheap insurance against a missed first notification.
    pub(super) async fn read_current_media(&self, attr: &GattCharacteristic) {
        for [entity, attribute] in ams::current_value_queries() {
            match read_attribute(attr, entity, attribute).await {
                Ok(value) => {
                    log::debug!(
                        "AMS read {} = {:?}",
                        ams::attribute_name(entity, attribute),
                        String::from_utf8_lossy(&value)
                    );
                    self.shared
                        .update_now_playing(|np| np.apply_attribute(entity, attribute, &value, now_ms()));
                }
                Err(e) => log::debug!(
                    "AMS read {} failed: {}",
                    ams::attribute_name(entity, attribute),
                    describe_error(&e)
                ),
            }
        }
        log::info!("AMS current values read");
    }

    /// One Entity Update notification. A truncated value (a long title) is read in full
    /// through Entity Attribute, as the spec intends; the cut value is the fallback.
    pub(super) async fn on_media_entity(&mut self, data: &[u8]) {
        let Some(update) = ams::EntityUpdate::parse(data) else {
            return log::debug!("AMS update too short to decode: {data:02X?}");
        };
        log::debug!("AMS update {update}");
        // No extra read while a stalled adapter settles: the cut value will do.
        let attr = self
            .link
            .as_ref()
            .filter(|_| !self.wedged())
            .and_then(|l| l.media.as_ref())
            .and_then(|m| m.entity_attribute.clone());
        let full = match (update.truncated, attr) {
            (true, Some(attr)) => {
                let read = read_attribute(&attr, update.entity, update.attribute).await;
                self.note_gatt(&read);
                match read {
                    Ok(value) => Some(value),
                    Err(e) => {
                        log::debug!(
                            "AMS full value of {} unavailable: {}",
                            ams::attribute_name(update.entity, update.attribute),
                            describe_error(&e)
                        );
                        None
                    }
                }
            }
            _ => None,
        };
        let value = full.as_deref().unwrap_or(update.value);
        self.shared
            .update_now_playing(|np| np.apply_attribute(update.entity, update.attribute, value, now_ms()));
    }

    /// The player's supported commands. iOS sends this only when the list changes (a new
    /// player, or the player enabling/disabling a command), so each one is worth a line.
    pub(super) fn on_media_commands(&self, data: &[u8]) {
        crate::bt_inventory::record_ams_commands(data);
        self.shared.update_now_playing(|np| {
            log::info!(
                "AMS supported commands (player {:?}): {} raw {data:?}",
                np.player.as_deref().unwrap_or("?"),
                ams::describe_commands(data)
            );
            np.apply_available_commands(data)
        });
    }

    /// Write one Remote Command. A successful write only means iOS handed it to the player,
    /// not that the player acted on it, so the log notes whether the player listed it.
    pub(super) async fn send_media_command(
        &mut self,
        command: ams::RemoteCommand,
        requested_at: std::time::Instant,
    ) -> Result<(), String> {
        // One press, one command: a repeat that queued behind the previous write is the same press.
        if !self.media_gate.admit(command, requested_at) {
            log::debug!(
                "dropped duplicate AMS command {} (pressed again before the last one went through)",
                command.as_str()
            );
            return Ok(());
        }
        let np = self.shared.now_playing();
        let waited = requested_at.elapsed();
        let context = format!(
            "player {:?}, listed as supported: {}, repeat now: {:?}, waited {} ms",
            np.player.as_deref().unwrap_or("?"),
            if np.available.is_empty() {
                "not yet known".to_string()
            } else {
                np.lists(command).to_string()
            },
            np.repeat,
            waited.as_millis()
        );
        if self.wedged() {
            log::info!(
                "AMS command {} not sent: reconnecting after a Bluetooth stall",
                command.as_str()
            );
            return Err(RECONNECTING.into());
        }
        let Some(media) = self.link.as_ref().and_then(|l| l.media.as_ref()) else {
            log::info!(
                "AMS command {} not sent: media not set up ({context})",
                command.as_str()
            );
            return Err("Media controls aren't available right now".into());
        };
        let ch = media.remote_command.clone();
        let result = winrt::write(&ch, &[command.id()]).await;
        if self.note_gatt(&result) {
            return Err(RECONNECTING.into());
        }
        match result {
            Ok(()) => {
                self.media_gate.sent(command, std::time::Instant::now());
                log::info!(
                    "AMS command {} ({}) sent: ok ({context})",
                    command.as_str(),
                    command.id()
                );
                Ok(())
            }
            Err(e) => {
                log::info!(
                    "AMS command {} ({}) sent: error {} ({context})",
                    command.as_str(),
                    command.id(),
                    describe_error(&e)
                );
                if self.on_closed(&e) {
                    return Err(RECONNECTING.into());
                }
                Err(e.to_string())
            }
        }
    }

    pub(super) async fn setup_battery(&self, device: &BluetoothLEDevice, gen: u64) -> Result<Battery, BleError> {
        let svc = winrt::service(device, winrt::sig_uuid(BATTERY_SERVICE))
            .await?
            .ok_or(BleError::NotFound("Battery service"))?;
        if let Ok(all) = async {
            let res = winrt::bounded_for(
                winrt::DISCOVERY_TIMEOUT,
                svc.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
            )
            .await?;
            Ok::<_, BleError>(res.Characteristics()?)
        }
        .await
        {
            let uuids: Vec<String> = all
                .into_iter()
                .filter_map(|c| c.Uuid().ok())
                .map(|u| format!("{u:?}"))
                .collect();
            log::info!("battery service characteristics: {uuids:?}");
        }
        let level = winrt::characteristic(&svc, winrt::sig_uuid(BATTERY_LEVEL), "battery level").await?;
        let initial = winrt::read(&level).await?;
        let _ = self.tx.send(Event::Battery { gen, data: initial });
        let tx = self.tx.clone();
        let updates = winrt::subscribe(&level, move |data| {
            let _ = tx.send(Event::Battery { gen, data });
        })
        .await
        .map_err(|e| log::info!("battery notifications unavailable, showing last read value: {e}"))
        .ok();
        Ok(Battery {
            _service: svc,
            _level: updates,
        })
    }
}

/// Entity Attribute: select the pair, then read its full value. Holds the AMS turn across the
/// pair, so the inventory's own pairs (`inventory::ams_read`) can't interleave with it.
async fn read_attribute(attr: &GattCharacteristic, entity: u8, attribute: u8) -> Result<Vec<u8>, BleError> {
    let _turn = super::inventory::AMS_ATTRIBUTE_TURN.lock().await;
    winrt::write(attr, &[entity, attribute]).await?;
    winrt::read(attr).await
}

/// An error with its ATT code spelled out (and named, for the AMS-specific ones), for the log.
fn describe_error(e: &BleError) -> String {
    match e.att_code() {
        Some(code) => match ams::error_name(code) {
            Some(name) => format!("ATT 0x{code:02X} {name}: {e}"),
            None => format!("ATT 0x{code:02X}: {e}"),
        },
        None => format!("{e} (no ATT code)"),
    }
}
