//! Hardware probe for experimental hands-free dialing (`tug_lib::hfp`) and recent calls
//! (PBAP call history). Run it with tug closed, so the two don't compete for the phone.
//!
//!   cargo run --example hfp_probe                         # find the phone, open the hands-free link, close it
//!   cargo run --example hfp_probe -- --dial "+15551234567"  # …and place a call on the iPhone
//!   cargo run --example hfp_probe -- --dial "+15551234567" --hold 15   # keep the link 15 s after dialing
//!   cargo run --example hfp_probe -- --calls              # pull recent calls (PBAP) instead
//!   cargo run --example hfp_probe -- --calls --show       # …with names and numbers

/// Prints the library's log lines to stderr, including every AT command and reply.
struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Debug && m.target().starts_with("tug_lib")
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
    use std::time::Duration;
    use tug_lib::hfp::session::{run, HfpError};
    use tug_lib::map::session::{find_devices, pull_call_history};
    static LOGGER: StderrLog = StderrLog;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug));

    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let dial = value("--dial");
    let hold = Duration::from_secs(value("--hold").and_then(|s| s.parse().ok()).unwrap_or(5));
    let calls = args.iter().any(|a| a == "--calls");
    let show = args.iter().any(|a| a == "--show");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async move {
        let devices = match find_devices().await {
            Ok(d) => d,
            Err(e) => return println!("device lookup failed: {e}"),
        };
        println!("paired phones (with message access): {}", devices.len());
        for d in &devices {
            println!("  - {} ({})", d.name, d.id);
        }
        let Some(device) = devices.first() else {
            return println!("no phone found: pair the iPhone in Windows › Bluetooth first");
        };

        if calls {
            println!("pulling recent calls (PBAP) from {} ...", device.name);
            match pull_call_history(&device.id, 50).await {
                Ok(list) => {
                    println!("recent calls: {}", list.len());
                    for c in &list {
                        let who = if show {
                            format!("{:?} {:?}", c.name, c.number)
                        } else {
                            format!("name={} number={}", c.name.is_some(), c.number.is_some())
                        };
                        println!("  {:?} at {:?} {who}", c.direction, c.at);
                    }
                    if list.is_empty() {
                        println!("(empty: is Sync Contacts on for this PC under Settings › Bluetooth › ⓘ?)");
                    }
                }
                Err(e) => println!("recent calls failed: {e}"),
            }
            return;
        }

        match &dial {
            Some(n) => println!("opening the hands-free link to {} and dialing {n} ...", device.name),
            None => println!("opening the hands-free link to {} (no call) ...", device.name),
        }
        match run(&device.id, dial.as_deref(), hold).await {
            Ok(report) => {
                println!("hands-free link: OK");
                println!("  phone features (+BRSF): {:?}", report.ag_features);
                println!("  indicators: {:?}", report.indicators);
                if dial.is_some() {
                    println!("  dial accepted (OK): {}", report.dialed);
                    println!("  call progress seen: {:?}", report.progress);
                    println!(
                        "RESULT: the iPhone accepted the call. Is it ringing out on the phone, and where is the audio?"
                    );
                } else {
                    println!("RESULT: tug can open the hands-free link. Try --dial next.");
                }
            }
            Err(HfpError::Blocked) => println!(
                "RESULT: BLOCKED. Windows won't share the hands-free link. Is Phone Link connected to this iPhone? \
                 Try again with Phone Link closed (or the iPhone unlinked from it)."
            ),
            Err(e) => println!("RESULT: failed: {e}"),
        }
    });
}

#[cfg(not(windows))]
fn main() {
    eprintln!("hfp_probe needs Windows");
}
