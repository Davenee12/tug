//! ANCS: subscribing, the one-at-a-time attribute request pump, reassembling replies, storing notifications, actions, and the post-reconnect sweep.

use super::*;

/// How long after a call ends before asking the phone for Recents, so the call is logged.
const CALL_LOG_SETTLE: Duration = Duration::from_secs(3);

impl Actor {
    pub(super) async fn setup_ancs(&mut self, device: &BluetoothLEDevice, gen: u64) -> Result<Ancs, BleError> {
        let svc = winrt::service(device, guid(ancs::SERVICE))
            .await?
            .ok_or(BleError::NotFound("Notification service (ANCS)"))?;
        let control_point = winrt::characteristic(&svc, guid(ancs::CONTROL_POINT), "ANCS control point").await?;
        let ns = winrt::characteristic(&svc, guid(ancs::NOTIFICATION_SOURCE), "ANCS notification source").await?;
        let ds = winrt::characteristic(&svc, guid(ancs::DATA_SOURCE), "ANCS data source").await?;

        // New session id before subscribing: iOS immediately replays every
        // notification still on the phone, and those must map to this session.
        let session_id = format!("{}-{gen}", now_ms());
        self.shared.set_live_session(Some(session_id.clone()));
        if let Some(link) = self.link.as_mut() {
            link.session_id = Some(session_id);
        }

        // Data Source first, so no attribute response can arrive unheard.
        let tx = self.tx.clone();
        let data_source = winrt::subscribe(&ds, move |data| {
            let _ = tx.send(Event::DataSource { gen, data });
        })
        .await?;
        let tx = self.tx.clone();
        let notification_source = winrt::subscribe(&ns, move |data| {
            let _ = tx.send(Event::NotificationSource { gen, data });
        })
        .await?;
        log::info!(
            "ANCS subscribed (session {})",
            self.shared.live_session().unwrap_or_default()
        );
        // First authorization probe shortly after connecting, then every CCCD_CHECK_SECS.
        self.cccd_check_in = 2;
        Ok(Ancs {
            _service: svc,
            control_point,
            notification_source,
            data_source,
            requests: RequestQueue::default(),
            reassembler: ancs::Reassembler::default(),
            resume: None,
            meta: HashMap::new(),
            rows: HashMap::new(),
            asked_apps: HashSet::new(),
            sweep_after: Some(Instant::now() + REPLAY_SETTLE),
        })
    }

    /// Once the iPhone has finished replaying what's still on it (no new events for
    /// REPLAY_SETTLE and every detail fetched), close rows it didn't replay: they
    /// were cleared while tug was disconnected.
    pub(super) fn sweep_if_settled(&mut self) {
        let Some(link) = self.link.as_mut() else { return };
        let (Some(a), Some(session)) = (link.ancs.as_mut(), link.session_id.as_deref()) else {
            return;
        };
        let settled = a.sweep_after.is_some_and(|t| Instant::now() >= t) && a.requests.is_idle();
        if !settled {
            return;
        }
        a.sweep_after = None;
        // If some replayed notification couldn't be fetched, its row still carries
        // an old session id even though it's on the phone: don't guess, skip.
        if !a.requests.all_notifications_fetched() {
            log::warn!("skipping the cleared-while-away sweep: some notifications couldn't be fetched");
            return;
        }
        match self.shared.store.sweep_stale(session, now_ms()) {
            Ok(ids) => {
                if !ids.is_empty() {
                    log::info!("{} notification(s) were cleared while tug was away", ids.len());
                }
                for id in ids {
                    self.shared.notification_removed(id);
                }
            }
            Err(e) => log::error!("sweep_stale: {e}"),
        }
    }

    /// Ask iOS whether it is actually sharing notifications with this PC.
    pub(super) async fn probe_ancs_authorization(&mut self, control_point: &GattCharacteristic) {
        let shared = match winrt::write(control_point, &ancs::probe()).await {
            Err(BleError::Protocol(Some(ancs::ERR_INVALID_PARAMETER))) | Ok(()) => true,
            Err(BleError::Protocol(Some(ancs::ATT_WRITE_NOT_PERMITTED))) => false,
            Err(e) => {
                log::warn!("ANCS authorization probe failed: {e}");
                return;
            }
        };
        let before = self.shared.status().services.notifications;
        if shared != before {
            log::info!(
                "ANCS authorization probe: iPhone {} notifications",
                if shared { "is sharing" } else { "is NOT sharing" }
            );
        } else {
            log::debug!("ANCS authorization probe: sharing={shared}");
        }
        self.shared.update_status(|s| {
            s.services.notifications = shared;
            if !shared {
                s.last_error = Some(NOT_SHARING.into());
            } else if s.last_error.as_deref() == Some(NOT_SHARING) {
                s.last_error = None;
            }
        });
    }

    pub(super) async fn on_notification_source(&mut self, data: &[u8]) {
        let ev = match ancs::parse_notification_source(data) {
            Ok(ev) => ev,
            Err(e) => return log::warn!("bad ANCS notification source packet: {e}"),
        };
        let Some(link) = self.link.as_mut() else {
            return log::warn!("ANCS event with no link");
        };
        let (Some(a), Some(session)) = (link.ancs.as_mut(), link.session_id.as_deref()) else {
            return log::debug!("ANCS event while not subscribed (link down or resubscribing); dropped");
        };
        if a.sweep_after.is_some() {
            a.sweep_after = Some(Instant::now() + REPLAY_SETTLE);
        }
        match ev.event {
            EventId::Added | EventId::Modified => {
                a.meta.insert(ev.uid, (ev.flags, ev.category));
                a.requests.push(Request::Notification(ev.uid));
            }
            EventId::Removed => {
                // A ringing call that stopped (answered, declined, rang out): it's in Recents now.
                if let Some((_, Category::IncomingCall)) = a.meta.remove(&ev.uid) {
                    if let Some(map) = self.shared.map.get() {
                        map.refresh_calls(CALL_LOG_SETTLE);
                    }
                }
                a.rows.remove(&ev.uid);
                a.requests.forget_notification(ev.uid);
                match self.shared.store.mark_removed(session, ev.uid, now_ms()) {
                    Ok(Some(id)) => self.shared.notification_removed(id),
                    Ok(None) => {}
                    Err(e) => log::error!("mark_removed: {e}"),
                }
            }
        }
        self.pump().await;
    }

    pub(super) async fn on_data_source(&mut self, data: &[u8]) {
        let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) else {
            return;
        };
        let mut result = a.reassembler.push(data);
        // A late reply to a request that already timed out: it names a notification
        // we did ask about, so its data is still good. Read it, then go back to
        // listening for the request that's actually in flight. (It can arrive while
        // we expect another notification's reply or an app-name reply.)
        let late = match result {
            Err(ParseError::UidMismatch { got, .. }) => Some(got),
            Err(ParseError::UnexpectedCommand { .. }) => ancs::notification_response_uid(data),
            _ => None,
        };
        if let Some(got) = late {
            if a.meta.contains_key(&got) {
                log::info!("late ANCS reply for {got}; accepting it");
                if a.resume.is_none() {
                    a.resume = a.requests.inflight().cloned();
                }
                a.reassembler.expect_notification(got);
                result = a.reassembler.push(data);
            }
        }
        let resp = match result {
            Ok(None) => return,
            Ok(Some(resp)) => {
                let done = match &resp {
                    Response::Notification { uid, .. } => Request::Notification(*uid),
                    Response::App { app_id, .. } => Request::App(app_id.clone()),
                };
                a.requests.complete(&done);
                if let Some(next) = a.resume.take() {
                    if a.requests.inflight() == Some(&next) {
                        expect(&mut a.reassembler, &next);
                        a.requests.touch(Instant::now());
                    }
                }
                Some(resp)
            }
            Err(e) => {
                // Garbled or unexpected data. Don't abandon the request in flight —
                // its real reply may still come; listen again and let the timeout decide.
                log::warn!("ANCS data source: {e}");
                a.resume = None;
                if let Some(cur) = a.requests.inflight().cloned() {
                    expect(&mut a.reassembler, &cur);
                }
                None
            }
        };
        if let Some(resp) = resp {
            self.on_response(resp);
        }
        self.pump().await;
    }

    pub(super) fn on_response(&mut self, resp: Response) {
        let Some(link) = self.link.as_mut() else { return };
        let (Some(a), Some(session)) = (link.ancs.as_mut(), link.session_id.as_deref()) else {
            return;
        };
        match resp {
            Response::Notification { uid, attrs } => {
                // Dismissed on the phone while its details were on the way: storing them now
                // would bring back a notification the user just cleared.
                let Some(&(flags, category)) = a.meta.get(&uid) else {
                    return log::debug!("dropping details for {uid}: removed while they were being fetched");
                };
                let stored = self.shared.store.upsert_notification(&NewNotification {
                    session,
                    uid,
                    category,
                    flags,
                    attrs: &attrs,
                    received_at: now_ms(),
                });
                match stored {
                    // Re-sent by the phone, but part of a conversation deleted in tug.
                    Ok(n) if n.hidden => {
                        a.rows.insert(uid, n.id);
                    }
                    Ok(n) => {
                        // A new text: pull it (and anything else new) over MAP right away.
                        if n.app_id == "com.apple.MobileSMS" {
                            if let Some(map) = self.shared.map.get() {
                                map.refresh();
                            }
                        }
                        // A new missed call: Recents has it (and the ringing call's removal may
                        // have come first, or not at all if tug connected mid-ring).
                        if category == Category::MissedCall && !flags.pre_existing {
                            if let Some(map) = self.shared.map.get() {
                                map.refresh_calls(CALL_LOG_SETTLE);
                            }
                        }
                        a.rows.insert(uid, n.id);
                        if n.app_name.is_none() && !attrs.app_id.is_empty() && a.asked_apps.insert(attrs.app_id.clone())
                        {
                            a.requests.push(Request::App(attrs.app_id.clone()));
                        }
                        self.shared.emit(events::NOTIFICATION, n);
                    }
                    Err(e) => log::error!("store notification: {e}"),
                }
            }
            Response::App {
                app_id,
                display_name: Some(name),
            } => {
                if let Err(e) = self.shared.store.set_app_name(&app_id, &name) {
                    log::error!("store app name: {e}");
                }
                self.shared.emit(events::APP_NAME, AppName { app_id, app_name: name });
            }
            Response::App { .. } => {}
        }
    }

    /// Send the next queued Control Point request if none is in flight.
    pub(super) async fn pump(&mut self) {
        loop {
            let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) else {
                return;
            };
            let Some(req) = a.requests.start_next(Instant::now()) else {
                return;
            };
            expect(&mut a.reassembler, &req);
            let bytes = match &req {
                Request::Notification(uid) => ancs::get_notification_attributes(*uid),
                Request::App(app_id) => ancs::get_app_attributes(app_id),
            };
            let cp = a.control_point.clone();
            let result = winrt::write(&cp, &bytes).await;
            let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) else {
                return;
            };
            match result {
                Ok(()) => return,
                // 0xA2: the notification vanished before we asked. Nothing to fetch.
                Err(BleError::Protocol(Some(ancs::ERR_INVALID_PARAMETER))) => {
                    a.reassembler.reset();
                    a.requests.drop_inflight();
                }
                Err(e) => {
                    log::warn!("ANCS request {req:?} failed: {e}");
                    a.reassembler.reset();
                    if let Some(gave_up) = a.requests.fail_inflight() {
                        log::warn!("giving up on ANCS request {gave_up:?} after {MAX_ATTEMPTS} attempts");
                        a.gave_up_on(&gave_up);
                    }
                }
            }
        }
    }

    pub(super) async fn perform_action(&mut self, id: i64, positive: bool) -> Result<(), String> {
        let a = self
            .link
            .as_ref()
            .and_then(|l| l.ancs.as_ref())
            .ok_or_else(|| "iPhone isn't connected".to_string())?;
        let uid = a
            .rows
            .iter()
            .find_map(|(uid, row)| (*row == id).then_some(*uid))
            .ok_or_else(|| "That notification is no longer on the iPhone".to_string())?;
        let (flags, _) = a.meta.get(&uid).copied().unwrap_or_default();
        if (positive && !flags.positive_action) || (!positive && !flags.negative_action) {
            return Err("The iPhone doesn't offer that action for this notification".into());
        }
        let cp = a.control_point.clone();
        let what = if positive { "positive" } else { "clear" };
        let result = winrt::write(&cp, &ancs::perform_action(uid, positive)).await;
        if result.is_ok() {
            log::info!("asked the iPhone to {what} notification {uid} (row {id})");
        }
        result.map_err(|e| match e {
            BleError::Protocol(Some(ancs::ERR_ACTION_FAILED)) => "The iPhone couldn't perform that action".to_string(),
            BleError::Protocol(Some(ancs::ERR_UNKNOWN_COMMAND | ancs::ERR_INVALID_COMMAND)) => {
                "This iOS version doesn't support notification actions".to_string()
            }
            other => other.to_string(),
        })
    }
}

/// Point the reassembler at the response `req` will produce.
pub(super) fn expect(reassembler: &mut ancs::Reassembler, req: &Request) {
    match req {
        Request::Notification(uid) => reassembler.expect_notification(*uid),
        Request::App(app_id) => reassembler.expect_app(app_id),
    }
}
