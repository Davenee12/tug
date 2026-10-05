//! Hardware probe for the MAP client: finds the iPhone's Message Access Server,
//! connects, lists the inbox and fetches the newest message.
//!
//!   cargo run --example map_probe            # summary only (no message text)
//!   cargo run --example map_probe -- --show  # also print message text
//!   cargo run --example map_probe -- --send "+15551234567" "hello from tug"
//!   cargo run --example map_probe -- --mark-read <handle>    # SetMessageStatus, then re-list
//!   cargo run --example map_probe -- --mark-unread <handle>

/// Prints the library's log lines to stderr so protocol details show up here.
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

#[cfg(windows)]
fn main() {
    use tug_lib::map::session::{find_devices, MapError, MapSession};
    static LOGGER: StderrLog = StderrLog;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));

    let args: Vec<String> = std::env::args().skip(1).collect();
    let show = args.iter().any(|a| a == "--show");
    let pbap_only = args.iter().any(|a| a == "--pbap");
    let send = args
        .iter()
        .position(|a| a == "--send")
        .map(|i| (args[i + 1].clone(), args[i + 2].clone()));
    let mark = ["--mark-read", "--mark-unread"].iter().find_map(|flag| {
        args.iter()
            .position(|a| a == flag)
            .map(|i| (args[i + 1].clone(), *flag == "--mark-read"))
    });
    let redact = |s: &str| {
        if show {
            s.to_string()
        } else {
            format!("<{} chars>", s.chars().count())
        }
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async move {
        let devices = match find_devices().await {
            Ok(d) => d,
            Err(e) => return println!("device lookup failed: {e}"),
        };
        println!("paired devices with message access: {}", devices.len());
        for d in &devices {
            println!("  - {} ({})", d.name, d.id);
        }
        let Some(device) = devices.first() else {
            return println!("{}", MapError::NoDevice);
        };

        if pbap_only {
            println!("pulling contacts (PBAP) from {} ...", device.name);
            match tug_lib::map::session::pull_contacts(&device.id).await {
                Ok(entries) => println!(
                    "contacts: {} people, {} numbers",
                    entries.len(),
                    entries.iter().map(|e| e.numbers.len()).sum::<usize>()
                ),
                Err(e) => println!("contacts failed: {e}"),
            }
            return;
        }
        println!("connecting to {} ...", device.name);
        let mut session = match MapSession::connect(&device.id).await {
            Ok(s) => s,
            Err(MapError::Consent) => {
                return println!(
                "FORBIDDEN: iOS refused message access. On the iPhone: Settings > Bluetooth > (i) next to this PC > \
                     turn on Show Notifications, then run this again."
            )
            }
            Err(e) => return println!("connect failed: {e}"),
        };
        println!("connected");

        match session.update_inbox().await {
            Ok(()) => println!("UpdateInbox: ok"),
            Err(e) => println!("UpdateInbox: {e} (continuing)"),
        }

        // How far back the phone lets us page (ListStartOffset), newest first.
        for offset in [0u16, 10, 20, 50] {
            match session.list("inbox", 50, offset).await {
                Ok(msgs) => println!(
                    "inbox page at offset {offset}: {} message(s), {} .. {}",
                    msgs.len(),
                    msgs.first().map(|m| m.datetime.as_str()).unwrap_or("-"),
                    msgs.last().map(|m| m.datetime.as_str()).unwrap_or("-"),
                ),
                Err(e) => println!("inbox page at offset {offset}: {e}"),
            }
        }

        match session.list("inbox", 10, 0).await {
            Ok(msgs) => {
                println!("inbox: {} message(s)", msgs.len());
                for m in &msgs {
                    println!(
                        "  {} {} from {} [{}] read={} subject={}",
                        m.handle,
                        m.datetime,
                        redact(&m.sender_name),
                        m.msg_type,
                        m.read,
                        redact(&m.subject)
                    );
                }
                if mark.is_some() {
                    // Skip GetMessage so the probe touches only the one message.
                } else if let Some(first) = msgs.first() {
                    match session.get_message(&first.handle).await {
                        Ok(b) => println!(
                            "GetMessage {}: type={} from={} body={}",
                            first.handle,
                            b.msg_type,
                            redact(b.originator_address.as_deref().unwrap_or("")),
                            redact(&b.body)
                        ),
                        Err(e) => println!("GetMessage failed: {e}"),
                    }
                }
            }
            Err(e) => println!("list inbox failed: {e}"),
        }

        if let Some((handle, read)) = &mark {
            match session.set_read(handle, *read).await {
                Ok(()) => println!("SetMessageStatus {handle} read={read}: accepted (0xA0)"),
                Err(e) => println!("SetMessageStatus {handle} read={read}: {e}"),
            }
            // What the phone now reports for that message.
            match session.list("inbox", 10, 0).await {
                Ok(msgs) => match msgs.iter().find(|m| &m.handle == handle) {
                    Some(m) => println!("after: {} read={}", m.handle, m.read),
                    None => println!("after: {handle} no longer in the inbox window"),
                },
                Err(e) => println!("re-list failed: {e}"),
            }
        }

        match session.list("sent", 5, 0).await {
            Ok(msgs) => println!("sent folder: {} message(s)", msgs.len()),
            Err(e) => println!("list sent: {e}"),
        }

        if let Some((to, text)) = send {
            match session.push_message(&to, &text).await {
                Ok(handle) => println!("PushMessage: accepted by iPhone (handle {handle:?})"),
                Err(e) => println!("PushMessage failed: {e}"),
            }
        }
        session.disconnect().await;
        println!("done");
    });
}

#[cfg(not(windows))]
fn main() {
    eprintln!("map_probe needs Windows");
}
