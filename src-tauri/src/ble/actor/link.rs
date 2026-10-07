//! The connection to the chosen iPhone: opening it, reacting to connect/disconnect, and (re)building the GATT services.

use super::*;

impl Actor {
    pub(super) fn drop_link(&mut self) {
        // Nothing more may be asked of this link, and the reconnect's AMS reads need the turn.
        self.cancel_inventory();
        self.link_down_at = None;
        // Whatever stall was being recovered from, the link it was on is gone.
        self.wedge_relink_at = None;
        self.wedge.new_link();
        if let Some(link) = self.link.take() {
            let _ = link.device.Close();
        }
        self.shared.set_live_session(None);
        self.shared.update_status(|s| {
            s.battery = None;
            s.awaiting_phone_allow = false;
            // No link, so nothing is "connected but locked" any more (Forget, relink, device switch).
            s.awaiting_unlock = false;
            // Set again by `restart_link` when tug is rebuilding the link on its own.
            s.reconnecting = false;
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
        self.restart_link();
    }

    /// `relink` without its log line: drop the link and connect again on the next tick, showing
    /// "Reconnecting…" (tug is doing this on its own; the phone doesn't need the user) until the
    /// link is back or a few attempts have failed.
    pub(super) fn restart_link(&mut self) {
        self.drop_link();
        self.retry_in = 0;
        // An away phone stays shown as away: "Reconnecting…" would flip back on the next failure.
        if self.away_since.is_none() {
            self.shared.update_status(|s| {
                s.connection = ConnectionState::Connecting;
                s.reconnecting = true;
            });
        }
    }

    /// Windows closed tug's GATT objects (seen right after a link blip on some adapters, and when a
    /// service is unticked in Settings): nothing on this link works any more, so rebuild it now
    /// instead of waiting for the next subscription check. Returns whether it did.
    pub(super) fn on_closed(&mut self, e: &BleError) -> bool {
        if !e.is_closed() {
            return false;
        }
        self.relink("Windows closed tug's Bluetooth objects for the iPhone");
        true
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
        // Classic side gives "Jordan's iPhone", which the message service may have adopted).
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
        let linked = self.link.as_ref().is_some_and(|l| l.connected);
        // Windows says the link is down: its link-up event connects at once (`on_connection`), so
        // don't block the actor on discovery that can't succeed; poke it only now and then.
        let since_poke = self.last_poke.map(|t| t.elapsed());
        if link_policy::wait_for_link_up(self.link.is_some(), linked, since_poke) {
            let left = link_policy::LINK_DOWN_POKE.saturating_sub(since_poke.unwrap_or_default());
            self.retry_in = self
                .retry_in
                .max(u32::try_from(left.as_secs()).unwrap_or(u32::MAX).max(1));
            return;
        }
        log::debug!("connecting to {id}");
        if link_policy::shows_connecting(self.connect_failures, linked, self.away_since.is_some()) {
            self.shared
                .update_status(|s| s.connection = ConnectionState::Connecting);
        }
        if self.link.is_none() {
            match self.open_link(&id).await {
                Ok(link) => self.link = Some(link),
                Err(e) => {
                    self.fail_connect(&e);
                    return;
                }
            }
        }
        if !self.link.as_ref().is_some_and(|l| l.connected) {
            self.last_poke = Some(Instant::now());
        }
        // Don't let a phone that isn't there hold the actor (and every click) for the full 30 s: the
        // short budget only applies while Windows reports the link down. With the link up, setup
        // gets its normal limits — uncached discovery can take well over 10 s on this adapter, and a
        // short budget there would drop and rebuild a working link forever. The first connect after
        // adopting a phone keeps the full budget too (a fresh bond waits on "Allow").
        let link_down = !self.link.as_ref().is_some_and(|l| l.connected);
        let setup = if self.connected_since_adopt && link_down {
            tokio::time::timeout(link_policy::RECONNECT_DISCOVERY, self.setup_services())
                .await
                .unwrap_or(Err(BleError::TimedOut))
        } else {
            self.setup_services().await
        };
        match setup {
            Ok(()) => {
                if self.connect_failures > 0 {
                    log::info!("connected after {} failed attempt(s)", self.connect_failures);
                }
                self.connect_failures = 0;
                self.away_since = None;
                self.last_poke = None;
                self.connected_since_adopt = true;
                self.adopt_timeouts = 0;
                if let Some(l) = self.link.as_mut() {
                    l.connected = true;
                }
                // Bluetooth inventory: the first connection this run gets a report a little later,
                // off this path, once ANCS has settled.
                self.schedule_inventory();
                self.shared.update_status(|s| {
                    s.connection = ConnectionState::Connected;
                    s.last_error = None;
                    s.pairing_stale = false;
                    s.awaiting_unlock = false;
                    s.reconnecting = false;
                    s.away = false;
                });
                // The texts/contacts/calls side sits behind the same phone; a fresh BLE link is a
                // good moment to retry message access rather than waiting out its own backoff.
                if let Some(map) = self.shared.map.get() {
                    map.refresh();
                }
                self.read_model_number();
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
                if let Some(status) = self
                    .link
                    .as_ref()
                    .and_then(|l| l._gatt_session.as_ref())
                    .and_then(|g| g.SessionStatus().ok())
                {
                    log::debug!("connect failed with the GATT session {status:?}");
                }
                self.fail_connect(&e);
                // A timed-out attempt may have left Windows' device/session objects for this phone
                // stuck (switching back to a phone needed a restart): open fresh ones next time.
                if e.is_timeout() {
                    // drop_link forgets "unlock your iPhone" (right for Forget or a switch); this
                    // is the same phone, so keep what the failed attempt decided.
                    let awaiting_unlock = self.shared.status().awaiting_unlock;
                    self.drop_link();
                    self.shared.update_status(|s| s.awaiting_unlock = awaiting_unlock);
                    if !self.connected_since_adopt {
                        self.adopt_timeouts = self.adopt_timeouts.saturating_add(1);
                    }
                }
                // One refusal can be a glitch; two in a row means the phone dropped the bond. A newly
                // adopted phone that never connects and keeps timing out needs pairing again too.
                let stale = (e.is_stale_bond() && self.connect_failures >= 2)
                    || link_policy::suggests_pair_again(self.connected_since_adopt, self.adopt_timeouts);
                self.shared.update_status(|s| s.pairing_stale = stale);
            }
        }
    }

    pub(super) fn fail_connect(&mut self, e: &BleError) {
        self.connect_failures = self.connect_failures.saturating_add(1);
        let message = e.to_string();
        // A phone at the edge of range fails every few seconds all night: log the first
        // few, then every tenth.
        if self.connect_failures <= 3 || self.connect_failures.is_multiple_of(10) {
            log::info!("connect attempt failed ({} in a row): {message}", self.connect_failures);
        } else {
            log::debug!("connect attempt failed ({} in a row): {message}", self.connect_failures);
        }
        let status = self.shared.status();
        let before = link_policy::Before {
            linked: self.link.as_ref().is_some_and(|l| l.connected),
            awaiting_unlock: status.awaiting_unlock,
            away: self.away_since.is_some(),
            failures: self.connect_failures,
            reconnecting: status.reconnecting,
        };
        let after = link_policy::after_failure(before, e.failure());
        // The iPhone is connected but isn't offering ANCS — it's locked after a restart, or
        // mid-update before its first unlock. Those attempts can't succeed until it's
        // unlocked, so back off far and say so instead of a silent 30 s loop all night.
        if after.awaiting_unlock && !before.awaiting_unlock {
            log::info!(
                "the iPhone is connected but not sharing notifications yet (locked, or just restarted) — unlock it to reconnect"
            );
        }
        if after.away && self.away_since.is_none() {
            log::info!("the iPhone is away; waiting for it to come back");
            self.away_since = Some(Instant::now());
        }
        let (base, cap) = match after.backoff {
            link_policy::Backoff::Unlock => (UNLOCK_RETRY_SECS, MAX_UNLOCK_RETRY_SECS),
            link_policy::Backoff::Connected => (RETRY_CONNECTED_SECS, MAX_RETRY_SECS),
            link_policy::Backoff::Idle => (RETRY_IDLE_SECS, MAX_RETRY_SECS),
        };
        self.retry_in = retry_delay(base, self.connect_failures, cap);
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            if after.publish_error || s.last_error.is_none() {
                s.last_error = Some(message);
            }
            s.awaiting_unlock = after.awaiting_unlock;
            s.reconnecting = after.reconnecting;
            s.away = after.away;
        });
    }

    pub(super) async fn open_link(&mut self, id: &str) -> Result<Link, BleError> {
        // Off this thread, like the GATT operations: this runs right after a relink for a stalled
        // adapter, when Windows is most likely to block the call itself.
        let hid = HSTRING::from(id);
        let device = winrt::off_thread(winrt::DISCOVERY_TIMEOUT, move || {
            Ok(BluetoothLEDevice::FromIdAsync(&hid)?.join()?)
        })
        .await?;
        self.gen += 1;
        let gen = self.gen;
        let tx = self.tx.clone();
        device.ConnectionStatusChanged(
            &TypedEventHandler::<BluetoothLEDevice, windows::core::IInspectable>::new(move |d, _| {
                if let Some(d) = d.as_ref() {
                    let connected = d.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected);
                    // Stamp the edge here, not when the actor gets to it: the actor can be stuck in
                    // a bounded await for tens of seconds, and the blip debounce must measure how
                    // long the link was really down, not how quickly the queue was drained.
                    let _ = tx.send(Event::Connection {
                        gen,
                        connected,
                        at: Instant::now(),
                    });
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
        let for_session = device.clone();
        let gatt_session = match winrt::off_thread(winrt::DISCOVERY_TIMEOUT, move || {
            let session = GattSession::FromDeviceIdAsync(&for_session.BluetoothDeviceId()?)?.join()?;
            let _ = session.SetMaintainConnection(true);
            Ok(session)
        })
        .await
        {
            Ok(s) => Some(s),
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

    /// A connection edge from Windows, stamped (`at`) when Windows reported it.
    pub(super) async fn on_connection(&mut self, connected: bool, at: Instant) {
        if self.link.is_none() {
            return;
        }
        if connected {
            let up = classify_link_up(self.link_down_at.take(), at);
            // The link was down past the grace even if `tick` hasn't torn it down yet (the up beat
            // the next tick, or the actor was busy): a real outage, so the old ANCS session, rows and
            // meta must go and the reconnect's cleared-while-away sweep must run. Tear down before
            // marking the link connected, because finish_link_down bails on a connected link.
            if up == LinkUp::AfterOutage {
                self.finish_link_down();
            }
            if let Some(l) = self.link.as_mut() {
                l.connected = true;
            }
            // A down→up blip inside the grace: the GATT subscription (and the pre-existing
            // notifications iOS is replaying on it) survived, so keep ANCS instead of tearing it down
            // and dropping the replay. Seen on hardware: a reconnect blip lost every waiting notification.
            let have_ancs = self.link.as_ref().is_some_and(|l| l.ancs.is_some());
            if up == LinkUp::Blip && have_ancs {
                log::debug!("iPhone link blip (down then up); keeping notifications subscribed");
                // On some adapters Windows closes tug's GATT objects across a blip; the subscription
                // check notices (and relinks), so run it on the next tick instead of in 15 s.
                self.cccd_check_in = 0;
                // pump() holds requests back while the link is down; send any that queued meanwhile.
                self.pump().await;
                return;
            }
            log::info!("iPhone link up");
            if !have_ancs {
                // Normally connect right away. After repeated failures the link is probably
                // flapping at the edge of range: let it settle instead of retrying on every blip.
                // A locked phone keeps its unlock backoff: the link coming up doesn't unlock it.
                self.retry_in = link_policy::retry_on_link_up(
                    self.retry_in,
                    self.connect_failures >= FLAPPING_AFTER,
                    FLAP_SETTLE_SECS,
                    self.shared.status().awaiting_unlock,
                );
            }
            return;
        }
        // Defer the teardown: a momentary blip shouldn't drop ANCS or flap the UI. `tick` finishes
        // the disconnect (`finish_link_down`) once the link has stayed down past the blip grace.
        if let Some(l) = self.link.as_mut() {
            l.connected = false;
        }
        // Keep the first down's stamp: a repeated down event mustn't restart the grace.
        self.link_down_at.get_or_insert(at);
    }

    /// A link-down that outlasted the blip grace: really disconnect. Runs from `tick` once the
    /// down has lasted past the grace, or from `on_connection` when the link-up came after it.
    /// A shorter blip never gets here: its GATT subscription survives, so the ANCS session, its
    /// notification UIDs, rows and meta are all kept. After a real outage iOS starts a fresh ANCS
    /// session whose UIDs don't match the old ones, so drop the services and session here; the
    /// reconnect resubscribes, and its post-replay sweep clears rows dismissed while away.
    pub(super) fn finish_link_down(&mut self) {
        self.link_down_at = None;
        // A link-up arrived first (handled as a blip): nothing to tear down.
        if self.link.as_ref().is_some_and(|l| l.connected) {
            return;
        }
        log::info!("iPhone link down");
        // Timeouts from before a real outage say nothing about the link that comes back.
        self.wedge.new_link();
        self.cancel_inventory();
        if let Some(l) = self.link.as_mut() {
            l.ancs = None;
            l.media = None;
            l._battery = None;
            l.session_id = None;
        }
        self.shared.set_live_session(None);
        // Down past the blip grace: the phone is away until it connects again.
        self.away_since.get_or_insert_with(Instant::now);
        // Keep any backoff: a link going down mid-flap isn't a reason to hurry. A locked phone's
        // longer unlock backoff survives too (it's still locked; only "unlock" clears that).
        let awaiting_unlock = self.shared.status().awaiting_unlock;
        self.retry_in = link_policy::retry_after_link_down(
            self.retry_in,
            retry_delay(RETRY_CONNECTED_SECS, self.connect_failures, MAX_RETRY_SECS),
            awaiting_unlock,
        );
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            s.battery = None;
            s.reconnecting = false;
            s.away = true;
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
        // Optional, unless they show the link itself is gone: carrying on over a dead link (seen
        // after a wake) only waits out more timeouts before it's rebuilt anyway.
        let media = match self.setup_media(&device, gen).await {
            Ok(m) => Some(m),
            Err(e) if e.link_is_dead() => {
                log::info!("AMS setup found the link gone: {e}");
                return Err(e);
            }
            Err(e) => {
                log::info!("AMS unavailable: {e}");
                None
            }
        };
        let battery = match self.setup_battery(&device, gen).await {
            Ok(b) => Some(b),
            Err(e) if e.link_is_dead() => {
                log::info!("Battery Service setup found the link gone: {e}");
                return Err(e);
            }
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

    /// Read the phone's model identifier ("iPhone16,2") from its Device Information Service, once
    /// per connection, so the UI can picture the exact phone. Purely cosmetic, so it runs as its
    /// own task: the actor loop (notifications, reconnects) never waits on it, every WinRT step is
    /// bounded, and a missing service or failed read is only logged. The result comes back as
    /// `Event::Model`, stamped with this subscription so a value from a replaced link is ignored.
    pub(super) fn read_model_number(&self) {
        let Some(link) = self.link.as_ref() else { return };
        let (device, gen, tx) = (link.device.clone(), link.sub_gen, self.tx.clone());
        tokio::task::spawn_local(async move {
            let read = async {
                let svc = winrt::service(&device, winrt::sig_uuid(device_info::DEVICE_INFORMATION_SERVICE))
                    .await?
                    .ok_or(BleError::NotFound("Device Information service"))?;
                let ch = winrt::characteristic(&svc, winrt::sig_uuid(device_info::MODEL_NUMBER_STRING), "model number")
                    .await?;
                winrt::read(&ch).await
            };
            match read.await {
                Ok(data) => {
                    let _ = tx.send(Event::Model { gen, data });
                }
                Err(e) => log::debug!("iPhone model not available: {e}"),
            }
        });
    }

    /// The phone's Model Number String arrived: keep it (settings + status) if it's a real
    /// identifier and differs from what we had.
    pub(super) fn on_model_number(&mut self, data: &[u8]) {
        let Some(model) = device_info::parse_model_number(data) else {
            log::debug!("ignoring an unreadable iPhone model number ({} bytes)", data.len());
            return;
        };
        let current = self.shared.status().device.and_then(|d| d.model);
        if current.as_deref() == Some(model.as_str()) {
            return;
        }
        log::info!("the iPhone is a {model}");
        let _ = self.shared.store.set_setting(keys::DEVICE_MODEL, &model);
        self.shared.update_status(|s| {
            if let Some(d) = s.device.as_mut() {
                d.model = Some(model);
            }
        });
    }

    /// Another app on this PC (e.g. Phone Link) can turn the shared ANCS CCCDs off,
    /// which silently stops notifications. Read them back and re-enable if needed.
    pub(super) async fn verify_ancs_subscription(&mut self) {
        // Paused while a stalled adapter settles: these reads would only add to the pile-up.
        if self.wedged() {
            return;
        }
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
            let state = winrt::notify_enabled(&ch).await;
            if self.note_gatt(&state) {
                return;
            }
            match state {
                Ok(true) => log::debug!("ANCS {name}: notifications on"),
                Ok(false) => {
                    log::warn!("ANCS {name}: notifications were off on the iPhone; re-enabling");
                    let enabled = winrt::enable_notify(&ch).await;
                    if self.note_gatt(&enabled) {
                        return;
                    }
                    if let Err(e) = enabled {
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
            if self.wedged() {
                return;
            }
        }
        let slow = Duration::from_secs(CCCD_CHECK_SECS.into());
        if self.optional_retry_at.is_none_or(|t| t.elapsed() >= slow) {
            self.optional_retry_at = Some(Instant::now());
            self.retry_optional_services().await;
        }
    }

    /// After a wake: does the link still answer? Reads the ANCS Notification Source subscription back,
    /// bounded by `link_policy::WAKE_CHECK`.
    pub(super) async fn wake_check(&self) -> link_policy::WakeCheck {
        use link_policy::WakeCheck;
        let Some(ch) = self
            .link
            .as_ref()
            .and_then(|l| l.ancs.as_ref())
            .map(|a| a.notification_source.characteristic().clone())
        else {
            return WakeCheck::Failed;
        };
        match tokio::time::timeout(link_policy::WAKE_CHECK, winrt::notify_enabled(&ch)).await {
            Ok(Ok(_)) => WakeCheck::Answered,
            Ok(Err(e)) if e.is_closed() => WakeCheck::Closed,
            Ok(Err(e)) if e.is_timeout() => WakeCheck::TimedOut,
            // An unanswered link after sleep is dead even if Windows never says so: rebuild it.
            Ok(Err(e)) if e.link_is_dead() => WakeCheck::TimedOut,
            Ok(Err(_)) => WakeCheck::Failed,
            Err(_) => WakeCheck::TimedOut,
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
                Err(e) => {
                    log::debug!("media retry: {e}");
                    if self.on_closed(&e) {
                        return;
                    }
                }
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
                Err(e) => {
                    log::debug!("battery retry: {e}");
                    self.on_closed(&e);
                }
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
