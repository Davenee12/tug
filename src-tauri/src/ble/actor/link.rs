//! The connection to the chosen iPhone: opening it, reacting to connect/disconnect, and (re)building the GATT services.

use super::*;

impl Actor {
    pub(super) fn drop_link(&mut self) {
        if let Some(link) = self.link.take() {
            let _ = link.device.Close();
        }
        self.shared.set_live_session(None);
        self.shared.update_status(|s| {
            s.battery = None;
            s.awaiting_phone_allow = false;
            s.services = Services {
                messages: s.services.messages,
                ..Services::default()
            };
        });
        self.shared.update_now_playing(|np| {
            let changed = *np != NowPlaying::default();
            *np = NowPlaying::default();
            changed
        });
    }

    /// Drop the link and connect again on the next tick.
    pub(super) fn relink(&mut self, why: &str) {
        log::warn!("{why}; reconnecting");
        self.drop_link();
        self.retry_in = 0;
    }

    /// The phone's name as it is now: saved, and shown everywhere tug names the phone.
    pub(super) fn set_device_name(&mut self, name: &str) {
        let name = name.trim();
        let current = self.shared.status().device.map(|d| d.name);
        // iOS reports junk mid-rename (a "4" once); a one- or two-char name is never a real
        // iPhone, so keep the last good one instead of following it onto the wrong label.
        if !crate::device_kind::plausible_device_name(name) || current.as_deref() == Some(name) {
            return;
        }
        // Don't let the LE side's generic "iPhone" clobber a specific name we already have (the
        // Classic side gives "Dave's iPhone", which the message service may have adopted).
        if current
            .as_deref()
            .is_some_and(|cur| crate::device_kind::more_specific_name(name, cur))
        {
            return;
        }
        log::info!("the iPhone is now called {name}");
        let _ = self.shared.store.set_setting(keys::DEVICE_NAME, name);
        self.shared.update_status(|s| {
            if let Some(d) = s.device.as_mut() {
                d.name = name.to_string();
            }
        });
    }

    pub(super) async fn connect(&mut self) {
        let Some(id) = self.device_id.clone() else { return };
        log::debug!("connecting to {id}");
        self.shared
            .update_status(|s| s.connection = ConnectionState::Connecting);
        if self.link.is_none() {
            match self.open_link(&id).await {
                Ok(link) => self.link = Some(link),
                Err(e) => {
                    self.fail_connect(e.to_string());
                    return;
                }
            }
        }
        match self.setup_services().await {
            Ok(()) => {
                if self.connect_failures > 0 {
                    log::info!("connected after {} failed attempt(s)", self.connect_failures);
                }
                self.connect_failures = 0;
                if let Some(l) = self.link.as_mut() {
                    l.connected = true;
                }
                self.shared.update_status(|s| {
                    s.connection = ConnectionState::Connected;
                    s.last_error = None;
                    s.pairing_stale = false;
                });
            }
            Err(e) => {
                // setup_ancs names the new session before subscribing; if setup then failed,
                // nothing is subscribed under it, so notifications mustn't look live (their
                // actions would go nowhere).
                if let Some(l) = self.link.as_mut() {
                    l.session_id = None;
                }
                self.shared.set_live_session(None);
                self.shared.update_status(|s| s.awaiting_phone_allow = false);
                self.fail_connect(e.to_string());
                // One refusal can be a glitch; two in a row means the phone dropped the bond.
                let stale = e.is_stale_bond() && self.connect_failures >= 2;
                self.shared.update_status(|s| s.pairing_stale = stale);
            }
        }
    }

    pub(super) fn fail_connect(&mut self, message: String) {
        self.connect_failures = self.connect_failures.saturating_add(1);
        // A phone at the edge of range fails every few seconds all night: log the first
        // few, then every tenth.
        if self.connect_failures <= 3 || self.connect_failures.is_multiple_of(10) {
            log::info!("connect attempt failed ({} in a row): {message}", self.connect_failures);
        } else {
            log::debug!("connect attempt failed ({} in a row): {message}", self.connect_failures);
        }
        let linked = self.link.as_ref().is_some_and(|l| l.connected);
        let base = if linked { RETRY_CONNECTED_SECS } else { RETRY_IDLE_SECS };
        self.retry_in = retry_delay(base, self.connect_failures);
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            s.last_error = Some(message);
        });
    }

    pub(super) async fn open_link(&mut self, id: &str) -> Result<Link, BleError> {
        let device = winrt::bounded_for(
            winrt::DISCOVERY_TIMEOUT,
            BluetoothLEDevice::FromIdAsync(&HSTRING::from(id))?,
        )
        .await?;
        self.gen += 1;
        let gen = self.gen;
        let tx = self.tx.clone();
        device.ConnectionStatusChanged(
            &TypedEventHandler::<BluetoothLEDevice, windows::core::IInspectable>::new(move |d, _| {
                if let Some(d) = d.as_ref() {
                    let connected = d.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected);
                    let _ = tx.send(Event::Connection { gen, connected });
                }
                Ok(())
            }),
        )?;
        // A rename on the phone shows up without waiting for a reconnect.
        let tx = self.tx.clone();
        device.NameChanged(
            &TypedEventHandler::<BluetoothLEDevice, windows::core::IInspectable>::new(move |d, _| {
                if let Some(name) = d.as_ref().and_then(|d| d.Name().ok()) {
                    let _ = tx.send(Event::Name {
                        gen,
                        name: name.to_string(),
                    });
                }
                Ok(())
            }),
        )?;
        // Ask Windows to keep the link up and re-establish it when the phone returns.
        let gatt_session = match winrt::bounded_for(
            winrt::DISCOVERY_TIMEOUT,
            GattSession::FromDeviceIdAsync(&device.BluetoothDeviceId()?)?,
        )
        .await
        {
            Ok(s) => {
                let _ = s.SetMaintainConnection(true);
                Some(s)
            }
            Err(e) => {
                log::warn!("GattSession unavailable: {e}");
                None
            }
        };
        let connected = device.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
        if let Ok(name) = device.Name() {
            self.set_device_name(&name.to_string());
        }
        Ok(Link {
            gen,
            sub_gen: gen,
            device,
            _gatt_session: gatt_session,
            connected,
            session_id: None,
            ancs: None,
            media: None,
            _battery: None,
        })
    }

    pub(super) fn on_connection(&mut self, connected: bool) {
        log::info!("iPhone link {}", if connected { "up" } else { "down" });
        let Some(link) = self.link.as_mut() else { return };
        link.connected = connected;
        if connected {
            if link.ancs.is_none() {
                // Normally connect right away. After repeated failures the link is probably
                // flapping at the edge of range: let it settle instead of retrying on every blip.
                self.retry_in = if self.connect_failures < FLAPPING_AFTER {
                    0
                } else {
                    self.retry_in.min(FLAP_SETTLE_SECS)
                };
            }
            return;
        }
        // Services and notification UIDs don't survive a disconnect.
        link.ancs = None;
        link.media = None;
        link._battery = None;
        link.session_id = None;
        self.shared.set_live_session(None);
        // Keep any backoff: a link going down mid-flap isn't a reason to hurry.
        self.retry_in = retry_delay(RETRY_CONNECTED_SECS, self.connect_failures);
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            s.battery = None;
            s.services = Services {
                messages: s.services.messages,
                ..Services::default()
            };
        });
        self.shared.update_now_playing(|np| {
            let changed = *np != NowPlaying::default();
            *np = NowPlaying::default();
            changed
        });
    }

    pub(super) async fn setup_services(&mut self) -> Result<(), BleError> {
        let device = match self.link.as_ref() {
            Some(l) => l.device.clone(),
            None => return Err(BleError::Unreachable),
        };
        // Every (re)subscription gets a fresh stamp, so GATT events still queued
        // from before a disconnect can't be taken for this subscription's.
        self.gen += 1;
        let gen = self.gen;
        if let Some(l) = self.link.as_mut() {
            l.sub_gen = gen;
        }

        // ANCS is required; media and battery are optional extras.
        let ancs = self.setup_ancs(&device, gen).await?;
        let media = match self.setup_media(&device, gen).await {
            Ok(m) => Some(m),
            Err(e) => {
                log::info!("AMS unavailable: {e}");
                None
            }
        };
        let battery = match self.setup_battery(&device, gen).await {
            Ok(b) => Some(b),
            Err(e) => {
                log::info!("Battery Service unavailable: {e}");
                None
            }
        };
        log::info!(
            "iPhone services ready: notifications, media={}, battery={}",
            media.is_some(),
            battery.is_some()
        );
        self.shared.update_status(|s| {
            s.services = Services {
                notifications: true,
                media: media.is_some(),
                battery: battery.is_some(),
                messages: s.services.messages,
            }
        });
        if let Some(link) = self.link.as_mut() {
            link.ancs = Some(ancs);
            link.media = media;
            link._battery = battery;
        }
        Ok(())
    }

    /// Another app on this PC (e.g. Phone Link) can turn the shared ANCS CCCDs off,
    /// which silently stops notifications. Read them back and re-enable if needed.
    pub(super) async fn verify_ancs_subscription(&mut self) {
        let Some(a) = self.link.as_ref().filter(|l| l.connected).and_then(|l| l.ancs.as_ref()) else {
            return;
        };
        let control_point = a.control_point.clone();
        let idle = a.requests.inflight().is_none();
        let mut closed = false;
        for (name, ch) in [
            ("data source", a.data_source.characteristic().clone()),
            ("notification source", a.notification_source.characteristic().clone()),
        ] {
            match winrt::notify_enabled(&ch).await {
                Ok(true) => log::debug!("ANCS {name}: notifications on"),
                Ok(false) => {
                    log::warn!("ANCS {name}: notifications were off on the iPhone; re-enabling");
                    if let Err(e) = winrt::enable_notify(&ch).await {
                        log::warn!("ANCS {name}: re-enable failed: {e}");
                    }
                }
                Err(e) => {
                    closed |= e.is_closed();
                    log::warn!("ANCS {name}: couldn't read subscription state: {e}");
                }
            }
        }
        // Seen on hardware: unticking a service in the iPhone's Windows device properties closed
        // every GATT object while the link stayed "connected", so notifications (calls included)
        // and media silently stopped until tug was restarted. Rebuild the link instead.
        if closed {
            self.relink("Windows closed tug's Bluetooth objects for the iPhone");
            return;
        }
        // Only probe between requests so the probe can't interleave with a real response.
        if idle {
            self.probe_ancs_authorization(&control_point).await;
        }
        let slow = Duration::from_secs(CCCD_CHECK_SECS.into());
        if self.optional_retry_at.is_none_or(|t| t.elapsed() >= slow) {
            self.optional_retry_at = Some(Instant::now());
            self.retry_optional_services().await;
        }
    }

    /// Media and battery are optional at connect time; if they failed (e.g. Windows
    /// still held them for a previous process), keep trying while linked.
    pub(super) async fn retry_optional_services(&mut self) {
        let Some(link) = self.link.as_ref().filter(|l| l.connected) else {
            return;
        };
        let (device, gen) = (link.device.clone(), link.sub_gen);
        let (need_media, need_battery) = (link.media.is_none(), link._battery.is_none());
        if need_media {
            match self.setup_media(&device, gen).await {
                Ok(m) => {
                    log::info!("media service connected on retry");
                    if let Some(l) = self.link.as_mut() {
                        l.media = Some(m);
                    }
                    self.shared.update_status(|s| s.services.media = true);
                }
                Err(e) => log::debug!("media retry: {e}"),
            }
        }
        if need_battery {
            match self.setup_battery(&device, gen).await {
                Ok(b) => {
                    log::info!("battery service connected on retry");
                    if let Some(l) = self.link.as_mut() {
                        l._battery = Some(b);
                    }
                    self.shared.update_status(|s| s.services.battery = true);
                }
                Err(e) => log::debug!("battery retry: {e}"),
            }
        }
    }
}

/// Turn a watcher id into a `BluetoothLEDevice`. A phone paired through
/// Windows Settings appears as a Classic device; its LE side shares the address.
pub(super) async fn resolve_le_device(
    id: &str,
    transport: Option<Transport>,
) -> windows::core::Result<BluetoothLEDevice> {
    let hid = HSTRING::from(id);
    if transport != Some(Transport::Classic) {
        if let Ok(le) = BluetoothLEDevice::FromIdAsync(&hid)?.await {
            return Ok(le);
        }
    }
    let classic = BluetoothDevice::FromIdAsync(&hid)?.await?;
    BluetoothLEDevice::FromBluetoothAddressAsync(classic.BluetoothAddress()?)?.await
}
