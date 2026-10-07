//! WinRT half of the actionable pop-ups: show toast XML under an AppUserModelID, hear its
//! body and buttons being pressed (`ToastNotification.Activated`), and take toasts back out
//! of Action Center. Thin on purpose; everything decidable is in `xml.rs`. Public so the
//! `toast_probe` example can drive it without the rest of tug.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{IInspectable, Interface, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::{DateTime, IReference, PropertyValue, TypedEventHandler};
use windows::UI::Notifications::{
    NotificationSetting, ToastActivatedEventArgs, ToastFailedEventArgs, ToastNotification,
    ToastNotificationManager,
};

/// PowerShell's AppUserModelID: always registered, so an uninstalled (dev) build can show
/// toasts under it. They're labelled "Windows PowerShell".
pub const POWERSHELL_AUMID: &str = r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// Recent toasts are kept alive here so their Activated handlers stay registered for as
/// long as they're likely to be pressed, including from Action Center.
const KEEP: usize = 32;
static RECENT: Mutex<VecDeque<ToastNotification>> = Mutex::new(VecDeque::new());

pub struct Toast<'a> {
    pub aumid: &'a str,
    pub xml: &'a str,
    /// At most 64 characters.
    pub tag: &'a str,
    pub group: &'a str,
    /// Drop it from Action Center after this long (follow-up notes).
    pub expires_in: Option<Duration>,
}

/// Show `toast`. `on_activated` gets the pressed body's or button's arguments and, for a
/// reply, the typed text; Windows calls it on a thread-pool thread.
pub fn show<F>(toast: Toast<'_>, on_activated: F) -> windows::core::Result<()>
where
    F: Fn(String, Option<String>) + Send + 'static,
{
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast.xml))?;
    let notification = ToastNotification::CreateToastNotification(&doc)?;
    notification.SetTag(&HSTRING::from(toast.tag))?;
    notification.SetGroup(&HSTRING::from(toast.group))?;
    if let Some(after) = toast.expires_in {
        notification.SetExpirationTime(&expiry(after)?)?;
    }
    notification.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
        move |_, args| {
            let Some(args) = args.as_ref().and_then(|a| a.cast::<ToastActivatedEventArgs>().ok()) else {
                return Ok(());
            };
            let arguments = args.Arguments().map(|a| a.to_string()).unwrap_or_default();
            on_activated(arguments, reply_text(&args));
            Ok(())
        },
    ))?;
    notification.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(
        |_, args| {
            let code = args.as_ref().and_then(|a| a.ErrorCode().ok());
            log::warn!("Windows couldn't show a pop-up: {code:?}");
            Ok(())
        },
    ))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(toast.aumid))?.Show(&notification)?;
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    recent.push_back(notification);
    while recent.len() > KEEP {
        recent.pop_front();
    }
    Ok(())
}

/// The text typed into the reply box, if this was a reply.
fn reply_text(args: &ToastActivatedEventArgs) -> Option<String> {
    let input = args.UserInput().ok()?;
    let key = HSTRING::from(super::xml::REPLY_INPUT);
    if !input.HasKey(&key).ok()? {
        return None;
    }
    let value = input.Lookup(&key).ok()?;
    let text = value.cast::<IReference<HSTRING>>().ok()?.Value().ok()?;
    Some(text.to_string())
}

/// `after` from now, as the WinRT DateTime toasts expire at.
fn expiry(after: Duration) -> windows::core::Result<IReference<DateTime>> {
    // DateTime counts 100 ns ticks since 1601-01-01; Unix time starts 11 644 473 600 s later.
    const EPOCH_GAP_S: u64 = 11_644_473_600;
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default() + after;
    let ticks = (unix.as_secs() + EPOCH_GAP_S) as i64 * 10_000_000 + i64::from(unix.subsec_nanos() / 100);
    PropertyValue::CreateDateTime(DateTime { UniversalTime: ticks })?.cast()
}

/// Take one toast back (on screen and in Action Center).
/// Whether Windows has turned pop-ups off for `aumid`: for that app, for every app, or by
/// policy. A synchronous read of the notifier's setting, so it can't hang.
pub fn blocked(aumid: &str) -> windows::core::Result<bool> {
    let setting = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Setting()?;
    Ok(blocks(setting))
}

fn blocks(setting: NotificationSetting) -> bool {
    setting == NotificationSetting::DisabledForApplication
        || setting == NotificationSetting::DisabledForUser
        || setting == NotificationSetting::DisabledByGroupPolicy
}

pub fn remove(aumid: &str, tag: &str, group: &str) -> windows::core::Result<()> {
    ToastNotificationManager::History()?.RemoveGroupedTagWithId(
        &HSTRING::from(tag),
        &HSTRING::from(group),
        &HSTRING::from(aumid),
    )
}

/// Take back every toast in a group.
pub fn remove_group(aumid: &str, group: &str) -> windows::core::Result<()> {
    ToastNotificationManager::History()?.RemoveGroupWithId(&HSTRING::from(group), &HSTRING::from(aumid))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_turned_off_setting_counts_as_blocked() {
        use super::{blocks, NotificationSetting};
        assert!(!blocks(NotificationSetting::Enabled));
        assert!(blocks(NotificationSetting::DisabledForApplication));
        assert!(blocks(NotificationSetting::DisabledForUser));
        assert!(blocks(NotificationSetting::DisabledByGroupPolicy));
        assert!(!blocks(NotificationSetting::DisabledByManifest));
    }

    use super::*;
    use crate::toast::xml::{note_toast, notification_toast, ToastSpec};

    /// What `xml.rs` builds is well-formed to Windows' own XML parser (which rejects the
    /// whole toast for one bad character).
    #[test]
    fn windows_parses_the_toast_xml() {
        let busy = ToastSpec {
            id: 1,
            title: "Messages · \"Zoe\" <3 & co".into(),
            body: "code 482913 \u{1}\u{1F697}".into(),
            name: "Zoe".into(),
            reply_to: Some("+1 (302) 555-0123".into()),
            mark_read: true,
            code: Some("482913".into()),
            call_back: true,
            clear: true,
        };
        for xml in [
            notification_toast(&busy, false),
            notification_toast(&ToastSpec::default(), true),
            note_toast(1, "Sent to Zoe", "on my way & <b>", true),
        ] {
            let doc = XmlDocument::new().unwrap();
            doc.LoadXml(&HSTRING::from(&xml))
                .unwrap_or_else(|e| panic!("{e}: {xml}"));
            ToastNotification::CreateToastNotification(&doc).unwrap();
        }
    }

    #[test]
    fn expiry_is_in_the_future() {
        let now = expiry(Duration::ZERO).unwrap().Value().unwrap().UniversalTime;
        let later = expiry(Duration::from_secs(600)).unwrap().Value().unwrap().UniversalTime;
        assert!((later - now - 6_000_000_000).abs() < 10_000_000, "{now} {later}");
        // 2020-01-01 in DateTime ticks: a sanity floor for the epoch arithmetic.
        assert!(now > 132_223_104_000_000_000);
    }
}
