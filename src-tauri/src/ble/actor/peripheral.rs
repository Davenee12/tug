//! Being findable: the radio watch and the connectable tug GATT service the iPhone connects to.

use super::*;

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
        radio.StateChanged(&TypedEventHandler::<Radio, windows::core::IInspectable>::new(
            move |r, _| {
                if let Some(r) = r.as_ref() {
                    let _ = tx.send(Event::Radio(map(r)));
                }
                Ok(())
            },
        ))?;
        self._radio = Some(radio);
        Ok(())
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
        match self.create_provider().await {
            Ok(provider) => {
                self.provider = Some(provider);
                self.advertise_failures = 0;
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
            Err(e) => {
                log::error!("advertising failed: {e}");
                self.shared.update_status(|s| {
                    s.advertising = AdvertisingState::Error;
                    s.last_error = Some(format!("Couldn't advertise to the iPhone: {e}"));
                });
            }
        }
    }

    pub(super) async fn create_provider(&self) -> Result<GattServiceProvider, BleError> {
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
                    let _ = tx.send(Event::Advertising(status));
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
