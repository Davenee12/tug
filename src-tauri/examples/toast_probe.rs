//! Probe for tug's actionable pop-ups (`tug_lib::toast`), no iPhone needed: shows a sample
//! toast the way tug builds it and prints every press (arguments, decoded action, typed
//! reply) so activation can be checked on the pop-up, from Action Center, and after exit.
//!
//!   cargo run --example toast_probe                     # a text with Reply / Mark read, under PowerShell's AUMID
//!   cargo run --example toast_probe -- --aumid tug      # …under the installed tug's AUMID (dev.davejames.tug)
//!   cargo run --example toast_probe -- --kind code      # one-time code: Copy code / Clear
//!   cargo run --example toast_probe -- --kind missed    # missed call: Call back / Clear
//!   cargo run --example toast_probe -- --kind plain     # body only
//!   cargo run --example toast_probe -- --wait 600       # keep listening 10 minutes (Action Center test)
//!   cargo run --example toast_probe -- --wait 0         # show and exit at once (the "tug isn't running" test)
//!
//! Nothing is sent anywhere: presses are only printed.

#[cfg(windows)]
fn main() {
    use std::time::Duration;
    use tug_lib::toast::native::{self, Toast, POWERSHELL_AUMID};
    use tug_lib::toast::xml::{decode, notification_toast, ToastSpec};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let aumid = match value("--aumid").as_deref() {
        None | Some("powershell") => POWERSHELL_AUMID.to_string(),
        Some("tug") => "dev.davejames.tug".to_string(),
        Some(other) => other.to_string(),
    };
    let wait: u64 = value("--wait").and_then(|w| w.parse().ok()).unwrap_or(120);
    let base = ToastSpec {
        id: 4242,
        title: "Messages · Tay".into(),
        body: "omw, 10 mins 🚗".into(),
        name: "Tay".into(),
        ..Default::default()
    };
    let spec = match value("--kind").as_deref() {
        Some("code") => ToastSpec {
            title: "Messages · Bank".into(),
            body: "Your code is 482913. Don't share it with anyone.".into(),
            name: "Bank".into(),
            code: Some("482913".into()),
            clear: true,
            ..base
        },
        Some("missed") => ToastSpec {
            title: "Phone · Mum".into(),
            body: "Missed Call".into(),
            name: "Mum".into(),
            call_back: true,
            clear: true,
            ..base
        },
        Some("plain") => base,
        _ => ToastSpec {
            reply_to: Some("+15555550123".into()),
            mark_read: true,
            clear: true,
            ..base
        },
    };
    let xml = notification_toast(&spec);
    println!("AUMID: {aumid}\nXML: {xml}\n");
    let toast = Toast {
        aumid: &aumid,
        xml: &xml,
        tag: "probe",
        group: "n",
        expires_in: None,
    };
    let shown = native::show(toast, |arguments, input| {
        println!("pressed: arguments={arguments:?}");
        println!("         action={:?}", decode(&arguments));
        println!("         typed={input:?}");
    });
    match shown {
        Ok(()) => println!("shown; listening for presses for {wait} s (Ctrl+C to stop)"),
        Err(e) => return println!("couldn't show the toast: {} ({:?})", e.message(), e.code()),
    }
    std::thread::sleep(Duration::from_secs(wait));
    println!("done; the toast stays in Action Center (press it now to see what happens with no listener)");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("toast_probe is Windows-only");
}
