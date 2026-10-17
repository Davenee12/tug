//! Being findable: the radio watch and the connectable tug GATT service the iPhone connects to.

use super::*;

/// The radio being watched, with its StateChanged handler: dropping it (when the radio is read
/// afresh, every `RADIO_RECHECK_SECS` while it isn't On) unregisters the handler, so they don't
/// pile up and deliver every change several times over.
pub(super) struct RadioWatch {
    radio: Radio,
    token: i64,
}

impl Drop for RadioWatch {
    fn drop(&mut self) {
        let _ = self.radio.RemoveStateChanged(self.token);
    }
}

impl Actor {
    pub(super) async fn watch_radio(&mut self) -> Result<(), BleError> {
        let radios = winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, Radio::GetRadiosAsync()?).await?;
        let Some(radio) = radios.into_iter().find(|r| r.Kind().ok() == Some(RadioKind::Bluetooth)) else {
            self.shared.update_status(|s| s.radio = RadioState::Unavailable);
            return Ok(());
        };
        let map = |r: &Radio| match r.State() {
            Ok(WinRadioState::On) => RadioState::On,
            Ok(WinRadioState::Off) => RadioState::Off,
            Ok(WinRadioState::Disabled) => RadioState::Unavailable,
            _ => RadioState::Unknown,
        };
        let state = map(&radio);
        log::info!("Bluetooth radio: {state:?}");
        self.shared.update_status(|s| s.radio = state);
        let tx = self.tx.clone();
        // The previous watch (and its handler) goes first, so there's only ever one.
        self._radio = None;
        let token = radio.StateChanged(&TypedEventHandler::<Radio, windows::core::IInspectable>::new(
            move |r, _| {
                if let Some(r) = r.as_ref() {
                    let _ = tx.send(Event::Radio(map(r)));
                }
                Ok(())
            },
        ))?;
        self._radio = Some(RadioWatch { radio, token });
        Ok(())
    }

    /// Bluetooth came (back) on: retry the connection at once, and restart advertising now if it's
    /// wanted and not running (Windows aborts it when the radio goes off, and its retries waited).
    pub(super) fn radio_came_on(&mut self) {
        self.retry_in = 0;
        self.connect_failures = 0;
        self.last_poke = None;
        self.advertise_failures = 0;
        let wanted = self.shared.store.setting(keys::ADVERTISE).ok().flatten().as_deref() != Some("false");
        if wanted && self.shared.status().advertising != AdvertisingState::On {
            self.advertise_retry_in = Some(0);
        }
    }

    /// Read the radio again (and watch it afresh) because it isn't On: a missed StateChanged mustn't
    /// leave a stale "off" in charge. Coming back On resets the connect backoff like the event does.
    pub(super) async fn recheck_radio(&mut self) {
        self.radio_recheck_in = link_policy::RADIO_RECHECK_SECS;
        let was = self.shared.status().radio;
        if let Err(e) = self.watch_radio().await {
            log::debug!("radio state still unavailable: {e}");
            return;
        }
        let now = self.shared.status().radio;
        if now != was {
            log::info!("Bluetooth radio re-read: {was:?} -> {now:?}");
            if now == RadioState::On {
                self.radio_came_on();
            }
        }
    }

    pub(super) async fn peripheral_supported(&self) -> Result<bool, BleError> {
        let adapter = winrt::bounded_for(winrt::DISCOVERY_TIMEOUT, BluetoothAdapter::GetDefaultAsync()?).await?;
        Ok(adapter.IsPeripheralRoleSupported()?)
    }

    pub(super) async fn start_advertising(&mut self) {
        if self.provider.is_some() {
            return;
        }
        self.shared
            .update_status(|s| s.advertising = AdvertisingState::Starting);
        self.advertise_gen += 1;
        match self.create_provider(self.advertise_gen).await {
            Ok(provider) => {
                self.provider = Some(provider);
                self.advertise_failures = 0;
            }
            // Bluetooth is off: there's nothing wrong with the adapter, and nothing to retry until
            // it's back (the radio coming On restarts it). Not an error, and no "may not support
            // peripheral mode" either.
            Err(e) if advertising_blocked(self.shared.status().radio) => {
                log::info!("advertising waits for Bluetooth to be on ({e})");
                self.advertise_retry_in = None;
                self.shared.update_status(|s| s.advertising = AdvertisingState::Off);
            }
            // The PC's adapter is busy or stalled (Windows aborts advertising when it blips, and
            // starting again can then hang). Nothing sends an Aborted event for a provider that was
            // never created, so retry from here, backing off, and keep it calm: it's not the
            // iPhone's doing and there's nothing for the user to do. Without this tug stayed
            // invisible, so the iPhone could never reconnect, until restarted.
            Err(e) if e.is_timeout() => {
                self.advertise_failures = self.advertise_failures.saturating_add(1);
                let wait = retry_delay(ADVERTISE_RETRY_SECS, self.advertise_failures, MAX_ADVERTISE_RETRY_SECS);
                log::warn!(
                    "advertising didn't start in time (Bluetooth busy, {} in a row); retrying in {wait}s",
                    self.advertise_failures
                );
                self.advertise_retry_in = Some(wait);
            }
            // Any other failure is retried too, on the same backoff: left alone it kept tug
            // invisible to the iPhone until restarted.
            Err(e) => {
                self.advertise_failures = self.advertise_failures.saturating_add(1);
                let wait = retry_delay(ADVERTISE_RETRY_SECS, self.advertise_failures, MAX_ADVERTISE_RETRY_SECS);
                log::error!(
                    "advertising failed ({} in a row): {e}; retrying in {wait}s",
                    self.advertise_failures
                );
                self.advertise_retry_in = Some(wait);
                self.shared.update_status(|s| {
                    s.advertising = AdvertisingState::Error;
                    s.last_error = Some(format!("Couldn't advertise to the iPhone: {e}"));
                });
            }
        }
    }

    /// `gen` stamps its status events (`Event::Advertising`).
    pub(super) async fn create_provider(&self, gen: u64) -> Result<GattServiceProvider, BleError> {
        // Bounded: this also runs from `tick` when Windows aborts advertising, which happens when the
        // adapter blips, and an unbounded wait there would park the whole actor.
        let res = winrt::bounded_for(
            winrt::DISCOVERY_TIMEOUT,
            GattServiceProvider::CreateAsync(guid(TUG_SERVICE))?,
        )
        .await?;
        let err = res.Error()?;
        if err != BluetoothError::Success {
            return Err(BleError::Win(windows::core::Error::new(
                windows::core::HRESULT(-1),
                format!("GATT service provider unavailable ({err:?}); this adapter may not support peripheral mode"),
            )));
        }
        let provider = res.ServiceProvider()?;

        let params = GattLocalCharacteristicParameters::new()?;
        params.SetCharacteristicProperties(GattCharacteristicProperties::Read)?;
        params.SetReadProtectionLevel(GattProtectionLevel::Plain)?;
        params.SetStaticValue(&winrt::to_buffer(b"tug")?)?;
        params.SetUserDescription(&HSTRING::from("tug"))?;
        winrt::bounded_for(
            winrt::DISCOVERY_TIMEOUT,
            provider.Service()?.CreateCharacteristicAsync(guid(TUG_INFO), &params)?,
        )
        .await?;

        let tx = self.tx.clone();
        provider.AdvertisementStatusChanged(&TypedEventHandler::<
            GattServiceProvider,
            GattServiceProviderAdvertisementStatusChangedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                if let Ok(status) = args.Status() {
                    let _ = tx.send(Event::Advertising { gen, status });
                }
            }
            Ok(())
        }))?;

        let adv = GattServiceProviderAdvertisingParameters::new()?;
        adv.SetIsConnectable(true)?;
        adv.SetIsDiscoverable(true)?;
        provider.StartAdvertisingWithParameters(&adv)?;
        Ok(provider)
    }

    pub(super) fn stop_advertising(&mut self) {
        self.advertise_retry_in = None;
        if let Some(p) = self.provider.take() {
            let _ = p.StopAdvertising();
        }
        self.shared.update_status(|s| s.advertising = AdvertisingState::Off);
    }
}
