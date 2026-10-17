use super::*;

fn win(class: &str, exe: &str, title: Option<&str>) -> TopWindow {
    TopWindow {
        class: class.into(),
        exe: exe.into(),
        title: title.map(Into::into),
    }
}

const MONITOR: Rect = Rect {
    left: 0,
    top: 0,
    right: 1920,
    bottom: 1080,
};

#[test]
fn only_a_real_full_screen_window_confirms_a_busy_shell() {
    // A borderless full-screen window covers the whole monitor.
    assert!(covers_monitor(MONITOR, MONITOR, "Chrome_WidgetWin_1"));
    // Windows 10/11 report invisible resize borders outside the rect: still covering.
    let bordered = Rect {
        left: -8,
        top: -8,
        right: 1928,
        bottom: 1088,
    };
    assert!(covers_monitor(bordered, MONITOR, "ApplicationFrameWindow"));
    // A maximized window stops at the taskbar.
    let maximized = Rect {
        left: -8,
        top: -8,
        right: 1928,
        bottom: 1040,
    };
    assert!(!covers_monitor(maximized, MONITOR, "Notepad"));
    // The desktop and taskbar cover the screen without being an app.
    for shell in ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
        assert!(!covers_monitor(MONITOR, MONITOR, shell), "{shell}");
    }
    // A second monitor to the right.
    let right = Rect {
        left: 1920,
        top: 0,
        right: 3840,
        bottom: 1080,
    };
    assert!(covers_monitor(right, right, "screenClass"));
    assert!(!covers_monitor(MONITOR, right, "screenClass"));
    assert!(
        !covers_monitor(MONITOR, Rect::default(), "x"),
        "an empty monitor rect never counts"
    );
}

#[test]
fn the_shell_state_counts_only_when_it_means_someone_may_be_watching() {
    assert_eq!(
        quns_signal(QUNS_PRESENTATION_MODE, false),
        Some(Signal::PresentationMode)
    );
    assert_eq!(quns_signal(QUNS_BUSY, true), Some(Signal::FullScreen));
    assert_eq!(quns_signal(QUNS_APP, true), Some(Signal::FullScreen));
    assert_eq!(
        quns_signal(QUNS_RUNNING_D3D_FULL_SCREEN, true),
        Some(Signal::FullScreenExclusive)
    );
    // "Busy" without a window that's really full screen: a stray answer, not a presentation.
    assert_eq!(quns_signal(QUNS_BUSY, false), None);
    assert_eq!(quns_signal(QUNS_APP, false), None);
    assert_eq!(quns_signal(QUNS_RUNNING_D3D_FULL_SCREEN, false), None);
    // Locked, the first hour after sign-in, or simply accepting notifications.
    for quiet in [QUNS_NOT_PRESENT, QUNS_QUIET_TIME, QUNS_ACCEPTS_NOTIFICATIONS, 0, 99] {
        assert_eq!(quns_signal(quiet, true), None, "{quiet}");
    }
}

#[test]
fn share_windows_match_by_program_and_class() {
    assert_eq!(
        window_signal(&win("ZPFloatToolbarClass", "zoom.exe", None)),
        Some(Signal::ShareWindow("zoom-share"))
    );
    assert_eq!(
        window_signal(&win("screenClass", "powerpnt.exe", None)),
        Some(Signal::SlideshowWindow("powerpoint-slideshow"))
    );
    assert_eq!(
        window_signal(&win("PodiumParent", "powerpnt.exe", None)),
        Some(Signal::SlideshowWindow("powerpoint-slideshow"))
    );
    // Another program's window with the same class name doesn't count.
    assert_eq!(window_signal(&win("screenClass", "someapp.exe", None)), None);
    assert_eq!(window_signal(&win("ZPFloatToolbarClass", "", None)), None);
    // Zoom's meeting window is there for the whole meeting, sharing or not.
    assert_eq!(window_signal(&win("ZPContentViewWndClass", "zoom.exe", None)), None);
}

#[test]
fn teams_and_browsers_need_their_share_bar_title() {
    let teams = |title| window_signal(&win("TeamsWebView", "ms-teams.exe", Some(title)));
    assert_eq!(
        teams("Sharing control bar | Microsoft Teams | Pinned window"),
        Some(Signal::ShareWindow("teams-share"))
    );
    assert_eq!(teams("Chat | General | Microsoft Teams"), None);
    assert_eq!(
        window_signal(&win(
            "Chrome_WidgetWin_1",
            "teams.exe",
            Some("Sharing control bar | Microsoft Teams")
        )),
        Some(Signal::ShareWindow("teams-share"))
    );

    let chrome = |exe, title| window_signal(&win("Chrome_WidgetWin_1", exe, Some(title)));
    for title in [
        "meet.google.com is sharing your screen.",
        "meet.google.com is sharing your screen and audio.",
        "example.com is sharing a window.",
        "example.com is sharing a window and audio.",
    ] {
        assert_eq!(
            chrome("chrome.exe", title),
            Some(Signal::ShareWindow("browser-share")),
            "{title}"
        );
        assert_eq!(
            chrome("msedge.exe", title),
            Some(Signal::ShareWindow("browser-share")),
            "{title}"
        );
    }
    // An ordinary browser window, even one whose page talks about sharing.
    assert_eq!(
        chrome("chrome.exe", "Who is sharing your screen. - Google Chrome"),
        None
    );
    assert_eq!(chrome("msedge.exe", "Meet - abc-defg-hij - Microsoft Edge"), None);
    // Sharing one tab shows only that tab: never tug or its pop-ups.
    assert_eq!(chrome("chrome.exe", "meet.google.com is sharing a Chrome tab."), None);
    // The same title in another Chromium app (VS Code, Slack) isn't a browser's share bar.
    assert_eq!(chrome("code.exe", "meet.google.com is sharing your screen."), None);
    // A title rule never matches a window whose title wasn't read.
    assert_eq!(window_signal(&win("Chrome_WidgetWin_1", "chrome.exe", None)), None);
}

#[test]
fn titles_are_read_only_where_a_rule_needs_them() {
    assert!(needs_title("Chrome_WidgetWin_1", "chrome.exe"));
    assert!(needs_title("TeamsWebView", "ms-teams.exe"));
    assert!(
        !needs_title("Chrome_WidgetWin_1", "code.exe"),
        "other Chromium apps keep their titles"
    );
    assert!(!needs_title("ZPFloatToolbarClass", "zoom.exe"), "the class is enough");
    assert!(!needs_title("screenClass", "powerpnt.exe"));
    assert!(class_of_interest("ZPFloatToolbarClass"));
    assert!(!class_of_interest("Notepad"));
}

#[test]
fn signals_combine_with_sharing_named_first() {
    assert_eq!(evaluate(&Snapshot::default()), None);
    assert_eq!(
        evaluate(&Snapshot {
            quns: Some(QUNS_ACCEPTS_NOTIFICATIONS),
            ..Default::default()
        }),
        None
    );

    // A slideshow shared in Zoom, full screen: it's "sharing".
    let shared_slideshow = Snapshot {
        quns: Some(QUNS_BUSY),
        foreground_full_screen: true,
        duplicating: false,
        windows: vec![
            win("screenClass", "powerpnt.exe", None),
            win("ZPFloatToolbarClass", "zoom.exe", None),
        ],
    };
    let found = evaluate(&shared_slideshow);
    assert_eq!(found, Some(Signal::ShareWindow("zoom-share")));
    assert_eq!(found.map(Signal::reason), Some(Reason::Sharing));

    // A slideshow on a projector beside presenter view: not full screen here, still presenting.
    let projector = Snapshot {
        quns: Some(QUNS_ACCEPTS_NOTIFICATIONS),
        windows: vec![win("PodiumParent", "powerpnt.exe", None)],
        ..Default::default()
    };
    assert_eq!(evaluate(&projector).map(Signal::reason), Some(Reason::Presenting));

    // Duplicate display beats plain full screen.
    let duplicated = Snapshot {
        quns: Some(QUNS_BUSY),
        foreground_full_screen: true,
        duplicating: true,
        windows: vec![],
    };
    assert_eq!(evaluate(&duplicated), Some(Signal::DuplicateDisplay));

    // Full-screen video alone.
    let video = Snapshot {
        quns: Some(QUNS_BUSY),
        foreground_full_screen: true,
        ..Default::default()
    };
    assert_eq!(evaluate(&video).map(Signal::reason), Some(Reason::FullScreen));

    // The shell said busy but nothing covers the screen.
    let stray = Snapshot {
        quns: Some(QUNS_BUSY),
        foreground_full_screen: false,
        ..Default::default()
    };
    assert_eq!(evaluate(&stray), None);
}

#[test]
fn the_log_names_the_signal_and_nothing_else() {
    let zoom = Some(Signal::ShareWindow("zoom-share"));
    let teams = Some(Signal::ShareWindow("teams-share"));
    assert_eq!(transition(None, zoom).as_deref(), Some("hiding previews: zoom-share"));
    assert_eq!(
        transition(zoom, None).as_deref(),
        Some("showing previews again (zoom-share ended)")
    );
    assert_eq!(transition(None, None), None);
    assert_eq!(transition(zoom, zoom), None);
    // Same reason, different app: nothing the window shows changes, so no line.
    assert_eq!(transition(zoom, teams), None);
    assert_eq!(
        transition(Some(Signal::FullScreen), zoom).as_deref(),
        Some("still hiding previews: zoom-share (was full-screen)")
    );
}

#[test]
fn state_for_the_window() {
    assert_eq!(
        ShareState::from_signal(None),
        ShareState {
            active: false,
            reason: None
        }
    );
    assert_eq!(
        ShareState::from_signal(Some(Signal::DuplicateDisplay)),
        ShareState {
            active: true,
            reason: Some(Reason::Presenting)
        }
    );
    let json = serde_json::to_string(&ShareState::from_signal(Some(Signal::FullScreen))).unwrap();
    assert_eq!(json, r#"{"active":true,"reason":"fullScreen"}"#);
    let json = serde_json::to_string(&ShareState::default()).unwrap();
    assert_eq!(json, r#"{"active":false,"reason":null}"#);
}

#[test]
fn the_setting_is_on_unless_switched_off() {
    assert!(enabled_from_setting(None));
    assert!(enabled_from_setting(Some("true")));
    assert!(!enabled_from_setting(Some("false")));
}

fn text_from_mom() -> ToastSpec {
    ToastSpec {
        id: 42,
        title: "Messages · Mom".into(),
        body: "the test results came back, call me".into(),
        name: "Mom".into(),
        reply_to: Some("+15555550123".into()),
        mark_read: true,
        code: None,
        call_back: false,
        clear: true,
        replies: vec!["Calling you now".into(), "Call you later".into()],
        app: "Messages".into(),
        kind: ToastKind::Message,
        ..Default::default()
    }
}

#[test]
fn a_hidden_text_says_only_the_app() {
    let r = redact(&text_from_mom());
    assert_eq!(r.title, "Messages");
    assert_eq!(r.body, "New message");
    assert_eq!(r.name, "", "the sender is private too");
    assert_eq!(
        r.reply_to, None,
        "the reply box names them, and nobody should type a reply on screen"
    );
    assert!(r.mark_read && r.clear, "buttons that reveal nothing stay");
    assert_eq!(r.id, 42, "buttons still act on the same notification");
    let xml = crate::toast::xml::notification_toast(&r, true);
    assert!(r.replies.is_empty(), "suggested replies come from the text: never on a hidden pop-up");
    for private in ["Mom", "test results", "5555550123", "Reply", "Calling you now"] {
        assert!(!xml.contains(private), "{private} leaked into {xml}");
    }
}

#[test]
fn every_kind_has_its_own_plain_wording() {
    let base = text_from_mom();
    let notification = redact(&ToastSpec {
        title: "Mail · Dr. Lee".into(),
        body: "Your appointment".into(),
        app: "Mail".into(),
        kind: ToastKind::Notification,
        ..base.clone()
    });
    assert_eq!(
        (notification.title.as_str(), notification.body.as_str()),
        ("Mail", "New notification")
    );

    let ringing = redact(&ToastSpec {
        title: "Phone · Mom".into(),
        body: "Incoming Call".into(),
        app: "Phone".into(),
        kind: ToastKind::Call,
        reply_to: None,
        mark_read: false,
        clear: false,
        ..base.clone()
    });
    assert_eq!((ringing.title.as_str(), ringing.body.as_str()), ("Incoming call", ""));

    let missed = redact(&ToastSpec {
        title: "Phone · Mom".into(),
        body: "Missed Call".into(),
        app: "Phone".into(),
        kind: ToastKind::MissedCall,
        call_back: true,
        ..base.clone()
    });
    assert_eq!((missed.title.as_str(), missed.body.as_str()), ("Missed call", ""));
    assert!(missed.call_back, "Call back names nobody");

    // A code: Copy code stays (the code rides in the button's arguments, never on screen).
    let code = redact(&ToastSpec {
        title: "482193 copied, from Chase".into(),
        body: "Your code is 482193".into(),
        code: Some("482193".into()),
        ..base.clone()
    });
    assert_eq!(code.code.as_deref(), Some("482193"));
    assert!(!code.title.contains("482193") && !code.body.contains("482193"));

    // No app given: tug's name rather than an empty title.
    let anonymous = redact(&ToastSpec {
        app: "  ".into(),
        kind: ToastKind::Notification,
        ..base.clone()
    });
    assert_eq!(anonymous.title, "tug");
}

#[test]
fn tugs_own_pop_ups_are_left_alone() {
    let battery = ToastSpec {
        id: 2_000_000_000,
        title: "iPhone battery low".into(),
        body: "20% left. Time to charge it.".into(),
        kind: ToastKind::Info,
        ..Default::default()
    };
    assert_eq!(redact(&battery), battery);
}

#[test]
fn a_spec_that_doesnt_say_what_it_is_is_hidden_like_a_notification() {
    // An older window (or a forgotten field) mustn't mean "show everything".
    let spec: ToastSpec = serde_json::from_str(r#"{"id":5,"title":"Messages · Mom","body":"hi"}"#).unwrap();
    assert_eq!(spec.kind, ToastKind::Notification);
    let r = redact(&spec);
    assert_eq!((r.title.as_str(), r.body.as_str()), ("tug", "New notification"));
    let typed: ToastSpec =
        serde_json::from_str(r#"{"id":5,"title":"t","body":"b","app":"WhatsApp","kind":"missedCall"}"#).unwrap();
    assert_eq!(typed.kind, ToastKind::MissedCall);
    assert_eq!(typed.app, "WhatsApp");
}

#[test]
fn a_hidden_follow_up_drops_the_name_and_text() {
    assert_eq!(redact_note("Sent"), ("Sent".to_string(), String::new()));
}
