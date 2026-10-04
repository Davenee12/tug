//! Hardware probe for the MAP client: finds the iPhone's Message Access Server,
//! connects, lists the inbox and fetches the newest message.
//!
//!   cargo run --example map_probe            # summary only (no message text)
//!   cargo run --example map_probe -- --show  # also print message text
//!   cargo run --example map_probe -- --send "+15551234567" "hello from tug"

#[cfg(windows)]
fn main() {
    use tug_lib::map::session::{find_devices, MapError, MapSession};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let show = args.iter().any(|a| a == "--show");
    let send = args
        .iter()
        .position(|a| a == "--send")
        .map(|i| (args[i + 1].clone(), args[i + 2].clone()));
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

        match session.list("inbox", 10).await {
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
                if let Some(first) = msgs.first() {
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

        match session.list("sent", 5).await {
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
