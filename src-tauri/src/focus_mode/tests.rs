use super::*;

const ON: FocusSettings = FocusSettings {
    enabled: true,
    full_screen: true,
    meetings: true,
    coding: true,
};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// Feed `found` to `d` at every 2 s poll from `from` to `to` seconds (inclusive), returning each
/// change the window would hear, with its time.
fn poll(d: &mut Debounce, t0: Instant, from: u64, to: u64, found: Option<Reason>) -> Vec<(u64, FocusState)> {
    (from..=to)
        .step_by(2)
        .filter_map(|s| d.update(t0 + secs(s), found).map(|state| (s, state)))
        .collect()
}

const MEETING: FocusState = FocusState {
    active: true,
    reason: Some(Reason::Meeting),
};
const OFF: FocusState = FocusState {
    active: false,
    reason: None,
};

#[test]
fn settings_are_off_until_turned_on_then_full_screen_and_meetings_count() {
    let missing = FocusSettings::from_setting(None);
    assert!(!missing.enabled && !missing.watching());
    assert!(missing.full_screen && missing.meetings && !missing.coding);
    assert_eq!(FocusSettings::from_setting(Some("not json")), FocusSettings::default());
    // Turning it on keeps the defaults for switches the frontend didn't write.
    let on = FocusSettings::from_setting(Some(r#"{"enabled":true}"#));
    assert!(on.enabled && on.full_screen && on.meetings && !on.coding && on.watching());
    let none = FocusSettings::from_setting(Some(
        r#"{"enabled":true,"fullScreen":false,"meetings":false,"coding":false}"#,
    ));
    assert!(!none.watching(), "on with every switch off looks for nothing");
}

#[test]
fn the_reason_follows_the_switches_and_a_meeting_wins() {
    let all = Look {
        full_screen: true,
        meeting_apps: 1,
        coding: true,
    };
    assert_eq!(reason(&ON, &all), Some(Reason::Meeting));
    assert_eq!(
        reason(&FocusSettings { meetings: false, ..ON }, &all),
        Some(Reason::FullScreen)
    );
    assert_eq!(
        reason(
            &FocusSettings {
                meetings: false,
                full_screen: false,
                ..ON
            },
            &all
        ),
        Some(Reason::Coding)
    );
    assert_eq!(reason(&FocusSettings { enabled: false, ..ON }, &all), None);
    // Turning a reason's switch off ends a focus held for it (`win::set_settings`).
    assert!(ON.allows(Reason::Meeting));
    assert!(!FocusSettings { meetings: false, ..ON }.allows(Reason::Meeting));
    assert!(!FocusSettings { enabled: false, ..ON }.allows(Reason::FullScreen));
    assert_eq!(reason(&ON, &Look::default()), None);
    let coding_only = Look {
        coding: true,
        ..Look::default()
    };
    assert_eq!(reason(&FocusSettings::default(), &coding_only), None, "off by default");
    assert_eq!(
        reason(
            &FocusSettings {
                enabled: true,
                ..FocusSettings::default()
            },
            &coding_only
        ),
        None
    );
}

#[test]
fn coding_is_what_voice_typing_calls_an_editor_or_terminal() {
    for exe in [
        r"C:\Users\x\AppData\Local\Programs\Microsoft VS Code\Code.exe",
        "WindowsTerminal.exe",
        "pwsh.exe",
        "idea64.exe",
    ] {
        assert!(is_coding(exe), "{exe}");
    }
    for exe in ["chrome.exe", "WINWORD.EXE", "slack.exe", ""] {
        assert!(!is_coding(exe), "{exe}");
    }
}

#[test]
fn apps_in_use_counts_open_uses_except_tugs_own_and_left_over_ones() {
    let now = filetime_from_unix_ms(1_790_000_000_000);
    let min = 60 * TICKS_PER_SEC;
    let own = non_packaged_key(r"C:\Program Files\tug\tug.exe");
    assert_eq!(own, "C:#Program Files#tug#tug.exe");
    let entry = |key: &str, start: u64, stop: u64| CapabilityUse {
        key: key.into(),
        start,
        stop,
    };
    let entries = [
        // In a call now.
        entry("MSTeams_8wekyb3d8bbwe", now - 30 * min, 0),
        // The same program using the camera too: one app.
        entry("MSTeams_8wekyb3d8bbwe", now - 29 * min, 0),
        // Finished.
        entry(r"C:#Program Files#Zoom#bin#Zoom.exe", now - 90 * min, now - 60 * min),
        // Never used.
        entry("Microsoft.WindowsCamera_8wekyb3d8bbwe", 0, 0),
        // tug's voice typing, whatever the case of the path.
        entry(&own.to_uppercase(), now - min, 0),
        // Left "in use" by an app that closed badly two days ago.
        entry(r"C:#Games#old.exe", now - 48 * 60 * min, 0),
    ];
    assert_eq!(apps_in_use(&entries, &own, now), 1);
    assert_eq!(apps_in_use(&entries[2..], &own, now), 0);
}

#[test]
fn filetime_matches_windows_epoch() {
    // 1970-01-01 in FILETIME ticks, as Windows documents it.
    assert_eq!(filetime_from_unix_ms(0), 116_444_736_000_000_000);
    assert_eq!(filetime_from_unix_ms(1_500), 116_444_736_000_000_000 + 15_000_000);
}

#[test]
fn a_blip_never_starts_a_focus() {
    let t0 = Instant::now();
    let mut d = Debounce::default();
    // Full screen for 4 s (three looks), then gone: nothing is held, so nothing to sum up.
    assert!(poll(&mut d, t0, 0, 4, Some(Reason::FullScreen)).is_empty());
    assert!(poll(&mut d, t0, 6, 60, None).is_empty());
    assert_eq!(d.state(), OFF);
    // Coming back later starts the count again from zero.
    assert!(poll(&mut d, t0, 62, 66, Some(Reason::FullScreen)).is_empty());
    assert_eq!(poll(&mut d, t0, 68, 68, Some(Reason::FullScreen)).len(), 1);
}

#[test]
fn a_focus_starts_once_seen_for_a_few_seconds_and_follows_its_reason() {
    let t0 = Instant::now();
    let mut d = Debounce::default();
    assert_eq!(poll(&mut d, t0, 0, 10, Some(Reason::Meeting)), vec![(6, MEETING)]);
    // The meeting goes full screen and back: the reason follows, the focus stays.
    let full = FocusState {
        active: true,
        reason: Some(Reason::FullScreen),
    };
    assert_eq!(d.update(t0 + secs(12), Some(Reason::FullScreen)), Some(full));
    assert_eq!(d.update(t0 + secs(14), Some(Reason::Meeting)), Some(MEETING));
}

#[test]
fn a_quick_look_elsewhere_does_not_end_a_focus() {
    let t0 = Instant::now();
    let mut d = Debounce::default();
    poll(&mut d, t0, 0, 10, Some(Reason::FullScreen));
    assert!(d.state().active);
    // Alt-tab out for 10 s and back.
    assert!(poll(&mut d, t0, 12, 22, None).is_empty());
    assert!(poll(&mut d, t0, 24, 30, Some(Reason::FullScreen)).is_empty());
    // The 20 s grace starts over after that.
    let ended = poll(&mut d, t0, 32, 60, None);
    assert_eq!(ended, vec![(52, OFF)]);
}

#[test]
fn turning_it_off_ends_a_focus_at_once() {
    let t0 = Instant::now();
    let mut d = Debounce::default();
    assert_eq!(d.reset(), None, "nothing to end");
    poll(&mut d, t0, 0, 10, Some(Reason::Coding));
    assert_eq!(d.reset(), Some(OFF));
    // And a new focus needs the full wait again.
    assert!(poll(&mut d, t0, 12, 16, Some(Reason::Coding)).is_empty());
}

#[test]
fn the_log_names_the_category_and_a_count_only() {
    let full = FocusState {
        active: true,
        reason: Some(Reason::FullScreen),
    };
    assert_eq!(
        transition(OFF, MEETING, 2).as_deref(),
        Some("holding pop-ups (meeting, 2 app(s) using the microphone or camera)")
    );
    assert_eq!(
        transition(MEETING, OFF, 0).as_deref(),
        Some("showing pop-ups again (meeting ended)")
    );
    assert_eq!(
        transition(MEETING, full, 0).as_deref(),
        Some("still holding pop-ups (full-screen, was meeting)")
    );
    assert_eq!(transition(full, full, 0), None);
}

#[test]
fn the_state_serializes_for_the_window() {
    let json = serde_json::to_value(FocusState {
        active: true,
        reason: Some(Reason::FullScreen),
    })
    .unwrap();
    assert_eq!(json, serde_json::json!({ "active": true, "reason": "fullScreen" }));
    assert_eq!(
        serde_json::to_value(OFF).unwrap(),
        serde_json::json!({ "active": false, "reason": null })
    );
}
