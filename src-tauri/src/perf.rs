//! Large-history timings for the store (Sprint 4 measurement).
//!
//! Not part of the normal test run. Run with:
//! `cargo test --release --lib perf -- --ignored --nocapture`

use std::time::{Duration, Instant};

use crate::ancs::{Category, EventFlags, NotificationAttributes};
use crate::messages::{IncomingMessage, SOURCE_IPHONE_MAP};
use crate::store::{NewNotification, Store};

const NOTIFICATIONS: u32 = 50_000;
const MESSAGES: u32 = 10_000;
const CONTACTS: u32 = 500;
const APPS: [&str; 6] = [
    "com.apple.MobileSMS",
    "net.whatsapp.WhatsApp",
    "com.tinyspeck.chatlyio",
    "com.apple.mobilemail",
    "com.google.Gmail",
    "com.apple.mobilephone",
];
const WORDS: [&str; 12] = [
    "dinner", "tonight", "meeting", "running", "late", "call", "photo", "invoice", "flight", "lunch", "ok", "thanks",
];

fn text(i: u32) -> String {
    (0..8)
        .map(|k| WORDS[((i * 7 + k * 3) % 12) as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Median of `runs` timings of `f`.
fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> Duration {
    let mut v: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(f());
            t.elapsed()
        })
        .collect();
    v.sort();
    v[runs / 2]
}

fn report(label: &str, d: Duration) {
    println!("{label:<52} {:>9.2} ms", d.as_secs_f64() * 1000.0);
}

#[test]
#[ignore]
fn perf_large_history() {
    let dir = std::env::temp_dir().join(format!("tug-perf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("perf.db");
    let store = Store::open(&path).unwrap();

    let t = Instant::now();
    store.conn().execute_batch("BEGIN").unwrap();
    for i in 0..NOTIFICATIONS {
        let attrs = NotificationAttributes {
            app_id: APPS[(i % 6) as usize].into(),
            title: format!("Person {}", i % 300),
            subtitle: String::new(),
            message: text(i),
            date: Some(format!(
                "2026-{:02}-{:02}T{:02}:{:02}:00",
                1 + i % 9,
                1 + i % 28,
                i % 24,
                i % 60
            )),
            positive_label: String::new(),
            negative_label: "Clear".into(),
        };
        store
            .upsert_notification(&NewNotification {
                session: &format!("s{}", i / 500),
                uid: i,
                category: Category::Social,
                flags: EventFlags::default(),
                attrs: &attrs,
                received_at: 1_760_000_000_000 + i as i64 * 60_000,
            })
            .unwrap();
    }
    for i in 0..MESSAGES {
        let handle = format!("H{i}");
        let address = format!("+1302555{:04}", i % CONTACTS);
        let body = text(i + 3);
        store
            .insert_incoming(&IncomingMessage {
                source: SOURCE_IPHONE_MAP,
                handle: &handle,
                address: &address,
                sender_name: None,
                body: &body,
                sent_at: None,
                received_at: 1_760_000_000_000 + i as i64 * 300_000,
                unread_on_phone: false,
            })
            .unwrap();
    }
    store.conn().execute_batch("COMMIT").unwrap();
    let phonebook: Vec<(String, String, Option<String>)> = (0..CONTACTS)
        .map(|i| (format!("+1302555{i:04}"), format!("Contact {i}"), None))
        .collect();
    store.save_phonebook(&phonebook).unwrap();
    println!(
        "\nseeded {NOTIFICATIONS} notifications, {MESSAGES} messages, {CONTACTS} contacts in {:?}",
        t.elapsed()
    );
    println!(
        "db size: {:.1} MB",
        std::fs::metadata(&path).unwrap().len() as f64 / 1e6
    );
    drop(store);

    let t = Instant::now();
    let store = Store::open(&path).unwrap();
    report("open existing database", t.elapsed());
    let live = Some("s99");

    // Startup: what init() loads.
    report(
        "startup: recent notifications (100)",
        time(21, || store.recent(100, None, live).unwrap()),
    );
    report(
        "startup: recent messages (2000)",
        time(21, || store.recent_messages(2000).unwrap()),
    );
    report("startup: contacts", time(21, || store.contacts().unwrap()));
    let msgs = store.recent_messages(2000).unwrap();
    println!(
        "{:<52} {:>9.1} KB",
        "startup: messages payload (JSON)",
        serde_json::to_vec(&msgs).unwrap().len() as f64 / 1e3
    );
    let page = store.recent(100, None, live).unwrap();
    let deep = page.last().unwrap().id - 40_000;
    report(
        "scroll: page 400 deep",
        time(21, || store.recent(100, Some(deep), live).unwrap()),
    );

    // Search (what Ctrl+K runs per keystroke).
    for q in ["dinner", "flig", "zzzz", "whatsapp", "person 12"] {
        report(
            &format!("search notifications \"{q}\" (30)"),
            time(21, || store.search(q, 30, live).unwrap()),
        );
    }
    report(
        "search messages \"dinner\" (30)",
        time(21, || store.search_messages("dinner", 30).unwrap()),
    );
    report(
        "search contacts \"contact 4\" (30)",
        time(21, || store.search_contacts("contact 4", 30).unwrap()),
    );
    report(
        "search contacts \"5550\" (30)",
        time(21, || store.search_contacts("5550", 30).unwrap()),
    );

    // Hot paths while connected.
    let mut uid = 1_000_000;
    let attrs = NotificationAttributes {
        app_id: APPS[0].into(),
        title: "Person 1".into(),
        message: "new one".into(),
        ..Default::default()
    };
    report(
        "new notification (upsert)",
        time(21, || {
            uid += 1;
            store
                .upsert_notification(&NewNotification {
                    session: "s99",
                    uid,
                    category: Category::Social,
                    flags: EventFlags::default(),
                    attrs: &attrs,
                    received_at: 1,
                })
                .unwrap()
        }),
    );
    let replay = NotificationAttributes {
        app_id: APPS[1].into(),
        title: "Person 1".into(),
        message: text(1),
        date: Some("2026-02-02T01:01:00".into()),
        negative_label: "Clear".into(),
        ..Default::default()
    };
    let flags = EventFlags {
        pre_existing: true,
        ..Default::default()
    };
    report(
        "replayed notification (content match)",
        time(21, || {
            uid += 1;
            store
                .upsert_notification(&NewNotification {
                    session: "s100",
                    uid,
                    category: Category::Social,
                    flags,
                    attrs: &replay,
                    received_at: 1,
                })
                .unwrap()
        }),
    );
    let mut h = 0;
    report(
        "new text (insert_incoming, with relist guard)",
        time(21, || {
            h += 1;
            let handle = format!("N{h}");
            store
                .insert_incoming(&IncomingMessage {
                    source: SOURCE_IPHONE_MAP,
                    handle: &handle,
                    address: "+13025550001",
                    sender_name: None,
                    body: "dinner tonight",
                    sent_at: Some("20261005T120000"),
                    received_at: 2_000_000_000_000,
                    unread_on_phone: false,
                })
                .unwrap()
        }),
    );
    report("learn contacts", time(5, || store.learn_contacts().unwrap()));
    let t = Instant::now();
    let swept = store.sweep_stale("s101", 1).unwrap().len();
    report(&format!("sweep after reconnect ({swept} rows)"), t.elapsed());

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
