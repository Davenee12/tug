//! Actionable Windows pop-ups: reply to a text, mark it read, copy a one-time code, call
//! back a missed call or clear a notification, right from the toast, without opening tug.
//!
//! **Who decides what.** The frontend still decides *whether* a notification pops up
//! (toasts on/off, do-not-disturb, muted apps, the rate limit in `ToastLimiter`) and what
//! applies to it (the conversation's number, a code, whether it's clearable), and hands
//! over a [`ToastSpec`] via the `show_toast` command. This side builds the toast and does
//! the presses, because tauri-plugin-notification's toasts can't carry a text box or
//! buttons. If the native toast can't be shown, the plugin's plain one is shown instead.
//!
//! **How a press reaches tug.** Toasts are shown with `CreateToastNotifierWithId(AUMID)`
//! and every button is a foreground activation whose `arguments` encode the action
//! (`xml::encode`). tug handles `ToastNotification.Activated` in-process (it lives in the
//! tray, so it's normally running): the arguments say what to do and `UserInput["reply"]`
//! holds the typed reply. This works on the pop-up and, with the installed build's
//! registered AUMID, from Action Center while tug is still running.
//!
//! **When tug isn't running** there's nobody to hear the press: Windows starts tug from
//! its Start-menu shortcut (no COM activator, so the press and any typed text are lost)
//! and the window simply opens. To keep that from happening with stale buttons, tug takes
//! its notification pop-ups back when it quits, and when the phone clears a notification.

pub mod xml;

#[cfg(windows)]
pub mod native;

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use tauri::AppHandle;

pub use xml::ToastSpec;

/// Toasts tied to a phone notification (tag = row id). Removing this group takes back
/// exactly those and leaves follow-up notes alone.
#[cfg_attr(not(windows), allow(dead_code))]
const GROUP_NOTIFICATIONS: &str = "n";
/// Follow-ups ("Sent to Tay", "Couldn't send"): tag `note-<id>`.
#[cfg_attr(not(windows), allow(dead_code))]
const GROUP_NOTES: &str = "note";
/// A "Sent" note leaves Action Center on its own after this long.
#[cfg(windows)]
const SENT_NOTE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Told to the frontend after a press was carried out, so its state follows: what's been
/// seen and read, where to navigate. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToastPressed {
    pub kind: PressKind,
    pub id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PressKind {
    /// The body: tug is in front; show this notification.
    Open,
    /// Mark read: read the conversation (seen in tug, read and cleared on the phone).
    Read,
    /// A reply was sent: the conversation is read, as replying on the phone would make it.
    Replied,
    /// The code is on the clipboard: its notification has done its job.
    Copied,
    /// The phone is calling them back.
    CalledBack,
}

/// Recent specs, for naming people in follow-ups ("Sent to Tay").
const KEEP: usize = 32;
static SPECS: Mutex<VecDeque<ToastSpec>> = Mutex::new(VecDeque::new());

fn remember(spec: &ToastSpec) {
    let mut specs = SPECS.lock().unwrap_or_else(|e| e.into_inner());
    specs.retain(|s| s.id != spec.id);
    specs.push_back(spec.clone());
    while specs.len() > KEEP {
        specs.pop_front();
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn name_for(id: i64) -> Option<String> {
    let specs = SPECS.lock().unwrap_or_else(|e| e.into_inner());
    specs
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.trim().to_string())
        .filter(|n| !n.is_empty())
}

/// The AppUserModelID tug's toasts are shown under. Installed, it's the bundle identifier,
/// which tug's NSIS installer stamps on its Start-menu shortcut (that registration is what
/// lets an unpackaged app's toasts show and be activated). A dev build run from `target\`
/// has no such shortcut, so like tauri-plugin-notification it borrows PowerShell's.
#[cfg(windows)]
fn aumid(app: &AppHandle) -> String {
    let dev = tauri::utils::platform::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .is_some_and(|dir| dir.ends_with(r"target\debug") || dir.ends_with(r"target\release"));
    if dev {
        native::POWERSHELL_AUMID.to_string()
    } else {
        app.config().identifier.clone()
    }
}

/// Show toast XML natively; presses come back to [`on_activated`].
#[cfg(windows)]
fn show_native(
    app: &AppHandle,
    xml: &str,
    tag: &str,
    group: &str,
    expires_in: Option<std::time::Duration>,
) -> windows::core::Result<()> {
    let aumid = aumid(app);
    let toast = native::Toast {
        aumid: &aumid,
        xml,
        tag,
        group,
        expires_in,
    };
    let handle = app.clone();
    native::show(toast, move |arguments, input| on_activated(&handle, arguments, input))
}

/// Pop up one phone notification with the actions its spec allows.
pub fn show(app: &AppHandle, spec: ToastSpec) {
    remember(&spec);
    #[cfg(windows)]
    {
        let xml = xml::notification_toast(&spec);
        match show_native(app, &xml, &spec.id.to_string(), GROUP_NOTIFICATIONS, None) {
            Ok(()) => return,
            Err(e) => log::warn!("actionable pop-up failed ({}); showing a plain one", e.message()),
        }
    }
    plain(app, &spec.title, &spec.body);
}

/// The plugin's plain toast: no buttons, but it shows.
fn plain(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("pop-up not shown: {e}");
    }
}

/// The phone cleared this notification: take its pop-up back, so no stale buttons linger.
pub fn withdraw(app: &AppHandle, id: i64) {
    #[cfg(windows)]
    if let Err(e) = native::remove(&aumid(app), &id.to_string(), GROUP_NOTIFICATIONS) {
        log::debug!("pop-up {id} not withdrawn: {}", e.message());
    }
    #[cfg(not(windows))]
    let _ = (app, id);
}

/// tug is quitting: once it's gone nothing would hear a press (and a typed reply would be
/// lost), so take its notification pop-ups out of Action Center.
pub fn withdraw_all(app: &AppHandle) {
    #[cfg(windows)]
    if let Err(e) = native::remove_group(&aumid(app), GROUP_NOTIFICATIONS) {
        log::debug!("pop-ups not withdrawn: {}", e.message());
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn on_activated(app: &AppHandle, arguments: String, input: Option<String>) {
    let app = app.clone();
    match xml::decode(&arguments) {
        Some(action) => {
            tauri::async_runtime::spawn(async move { act(app, action, input).await });
        }
        None => {
            log::info!("pop-up pressed with arguments tug doesn't know; bringing tug forward");
            crate::tray::show(&app);
        }
    }
}

/// True the first time this reply (same notification, same text) is pressed within a minute.
fn first_reply(id: i64, text: &str) -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static RECENT_REPLIES: Mutex<Vec<(i64, String, Instant)>> = Mutex::new(Vec::new());
    let mut recent = RECENT_REPLIES.lock().unwrap_or_else(|e| e.into_inner());
    recent.retain(|(_, _, at)| at.elapsed() < Duration::from_secs(60));
    if recent.iter().any(|(i, t, _)| *i == id && t == text) {
        return false;
    }
    recent.push((id, text.to_string(), Instant::now()));
    true
}

#[cfg(windows)]
async fn act(app: AppHandle, action: xml::ToastAction, input: Option<String>) {
    use tauri::{Emitter, Manager};
    use xml::ToastAction;

    use crate::ble::Command;
    use crate::commands::AppState;

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (shared, ble) = (state.shared.clone(), state.ble.clone());
    let id = action.id();
    let pressed = |kind: PressKind| {
        if let Err(e) = app.emit(crate::state::events::TOAST_PRESSED, ToastPressed { kind, id }) {
            log::warn!("emit toast-pressed failed: {e}");
        }
    };
    // What the phone still knows about it (category), to refuse a press that doesn't fit.
    let category = || {
        shared
            .store
            .get(id, shared.live_session().as_deref())
            .ok()
            .flatten()
            .map(|n| n.category)
    };
    match action {
        ToastAction::Open { .. } => {
            crate::tray::show(&app);
            pressed(PressKind::Open);
        }
        ToastAction::Reply { to, .. } => {
            let who = name_for(id).unwrap_or_else(|| to.clone());
            let text = input.unwrap_or_default();
            if text.trim().is_empty() {
                // Send with nothing typed: open the conversation instead.
                crate::tray::show(&app);
                pressed(PressKind::Open);
                return;
            }
            let text = text.trim().to_string();
            // A double-press, or pressing Send again from Action Center before Windows dismissed
            // the pop-up, must not text them twice: take the pop-up down and drop a repeat.
            if !first_reply(id, &text) {
                log::info!("ignored a repeated reply press (row {id})");
                return;
            }
            withdraw(&app, id);
            let sent = match shared.map.get().cloned() {
                Some(map) => map.send(to, text.clone()).await.map(|_| ()),
                None => Err("Message service isn't running".to_string()),
            };
            match sent {
                Ok(()) => {
                    log::info!("sent a reply from a pop-up (row {id})");
                    note(&app, id, &format!("Sent to {who}"), &text, true);
                    pressed(PressKind::Replied);
                }
                Err(e) => {
                    log::info!("reply from a pop-up not sent (row {id}): {e}");
                    let body = format!("{e}\n\u{201c}{text}\u{201d}");
                    note(&app, id, &format!("Couldn't send to {who}"), &body, false);
                }
            }
        }
        ToastAction::Read { .. } => pressed(PressKind::Read),
        ToastAction::Copy { code, .. } => match copy(&app, code).await {
            Ok(()) => pressed(PressKind::Copied),
            Err(e) => note(&app, id, "Couldn't copy the code", &e, false),
        },
        ToastAction::CallBack { .. } => {
            let who = name_for(id).unwrap_or_else(|| "them".into());
            // Only a missed call's positive action is "Dial"; on a ringing call it would answer.
            if category().as_deref() != Some("missedCall") {
                return log::warn!("call back pressed on row {id}, which isn't a missed call; ignored");
            }
            match ble
                .request(|reply| Command::PerformAction {
                    id,
                    positive: true,
                    reply,
                })
                .await
            {
                Ok(()) => pressed(PressKind::CalledBack),
                Err(e) => note(&app, id, &format!("Couldn't call {who} back"), &e, false),
            }
        }
        ToastAction::Clear { .. } => {
            // A ringing call's negative action is Decline: only its own button sends that.
            if category().as_deref() == Some("incomingCall") {
                return log::warn!("clear pressed on ringing call row {id}; ignored");
            }
            if let Err(e) = ble
                .request(|reply| Command::PerformAction {
                    id,
                    positive: false,
                    reply,
                })
                .await
            {
                note(&app, id, "Couldn't clear it on your iPhone", &e, false);
            }
        }
    }
}

/// The clipboard wants the main (STA) thread, and presses arrive on a thread-pool thread.
#[cfg(windows)]
async fn copy(app: &AppHandle, code: String) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(crate::clipboard::set_text(&code));
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|_| "The clipboard didn't answer".to_string())?
}

/// A short follow-up about notification `id`. One per notification (a newer one replaces
/// it), in its own group so the phone clearing the notification doesn't take it back.
#[cfg(windows)]
fn note(app: &AppHandle, id: i64, title: &str, body: &str, quiet: bool) {
    let xml = xml::note_toast(id, title, body, quiet);
    let tag = format!("note-{id}");
    if let Err(e) = show_native(app, &xml, &tag, GROUP_NOTES, quiet.then_some(SENT_NOTE_TTL)) {
        log::warn!("follow-up pop-up failed ({}); showing a plain one", e.message());
        plain(app, title, body);
    }
}

#[cfg(test)]
mod reply_tests {
    #[test]
    fn a_repeated_reply_press_is_dropped() {
        assert!(super::first_reply(9_001, "on my way"));
        assert!(
            !super::first_reply(9_001, "on my way"),
            "same reply again within a minute"
        );
        assert!(
            super::first_reply(9_001, "actually 5 min"),
            "a different reply still goes"
        );
        assert!(
            super::first_reply(9_002, "on my way"),
            "another notification is separate"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the remembered specs are global, and parallel tests would evict each other's.
    #[test]
    fn follow_ups_name_the_person_from_recent_specs() {
        remember(&ToastSpec {
            id: 77,
            name: " Tay ".into(),
            ..Default::default()
        });
        assert_eq!(name_for(77).as_deref(), Some("Tay"));
        remember(&ToastSpec {
            id: 78,
            name: "   ".into(),
            ..Default::default()
        });
        assert_eq!(name_for(78), None);
        assert_eq!(name_for(-1), None);

        for id in 1000..1000 + KEEP as i64 + 5 {
            remember(&ToastSpec {
                id,
                name: format!("p{id}"),
                ..Default::default()
            });
        }
        assert_eq!(name_for(1000), None);
        assert_eq!(
            name_for(1000 + KEEP as i64 + 4).as_deref(),
            Some(&*format!("p{}", 1000 + KEEP as i64 + 4))
        );
        assert!(SPECS.lock().unwrap().len() <= KEEP);
    }

    #[test]
    fn presses_reach_the_frontend_in_camel_case() {
        let json = serde_json::to_string(&ToastPressed {
            kind: PressKind::CalledBack,
            id: 5,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"calledBack","id":5}"#);
    }
}
