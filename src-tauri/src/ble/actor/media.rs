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
        for registration in ams::registrations() {
            winrt::write(&entity, &registration).await?;
        }
        self.read_current_media(&svc).await;
        Ok(Media {
            _service: svc,
            remote_command: remote,
            _remote_updates: remote_updates,
            _entity_update: entity_updates,
        })
    }

    /// Pick up whatever is already playing; Entity Update only reports changes.
    pub(super) async fn read_current_media(&self, svc: &GattDeviceService) {
        let attr = match winrt::characteristic(svc, guid(ams::ENTITY_ATTRIBUTE), "AMS entity attribute").await {
            Ok(c) => c,
            Err(e) => return log::info!("AMS current values unavailable: {e}"),
        };
        for [entity, attribute] in ams::current_value_queries() {
            if let Err(e) = winrt::write(&attr, &[entity, attribute]).await {
                log::debug!("AMS select {entity}/{attribute} failed: {e}");
                continue;
            }
            match winrt::read(&attr).await {
                Ok(value) => self
                    .shared
                    .update_now_playing(|np| np.apply_attribute(entity, attribute, &value, now_ms())),
                Err(e) => log::debug!("AMS read {entity}/{attribute} failed: {e}"),
            }
        }
        log::info!("AMS current values read");
    }

    pub(super) async fn setup_battery(&self, device: &BluetoothLEDevice, gen: u64) -> Result<Battery, BleError> {
        let svc = winrt::service(device, winrt::sig_uuid(BATTERY_SERVICE))
            .await?
            .ok_or(BleError::NotFound("Battery service"))?;
        if let Ok(all) = async {
            svc.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?
                .await?
                .Characteristics()
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
