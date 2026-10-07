//! Bluetooth LE link to the iPhone.
//!
//! All Bluetooth state lives in one actor running on its own thread (WinRT
//! async types are not `Send`, so it gets a current-thread runtime). Tauri
//! commands talk to it through [`BleHandle`].

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::ams::RemoteCommand;
use crate::state::Shared;

#[cfg(windows)]
mod actor;
#[cfg(windows)]
pub(crate) mod winrt;

pub type Reply = oneshot::Sender<Result<(), String>>;

pub enum Command {
    PerformAction {
        id: i64,
        positive: bool,
        reply: Reply,
    },
    Media {
        command: RemoteCommand,
        /// When the press happened, so presses queued behind a stalled write can be told
        /// apart from deliberate ones (see `ams::CommandGate`).
        requested_at: std::time::Instant,
        /// A dropped repeat answers `Err(ams::REPEAT_IGNORED)` instead of `Ok` (for callers that
        /// report the outcome; a button or media key just treats the repeat as handled).
        report_repeat: bool,
        reply: Reply,
    },
    StartDiscovery,
    StopDiscovery,
    /// Re-run the iPhone inquiry without disturbing the LE watcher or the rows already listed, so a
    /// phone made discoverable after scanning began still shows up (an unpaired-Classic AEP watcher
    /// only inquires once).
    RescanDiscovery,
    Pair {
        id: String,
        reply: Reply,
    },
    /// Unpair a leftover phone's LE and Classic bonds (both, matched by name), then keep scanning so
    /// it reappears unpaired and ready to pair fresh.
    RemovePairing {
        id: String,
        reply: Reply,
    },
    UseDevice {
        id: String,
        reply: Reply,
    },
    /// Pair the iPhone's Classic (texts) side from inside tug (a one-shot inquiry during the
    /// setup step); the PIN shows in tug. Requires the LE (notifications) side first.
    PairTexts {
        reply: Reply,
    },
    Forget {
        reply: Reply,
    },
    SetAdvertising {
        enabled: bool,
        reply: Reply,
    },
    /// Run the Bluetooth inventory now (or join the one already running); see `crate::bt_inventory`.
    Inventory {
        reply: oneshot::Sender<crate::bt_inventory::BtInventory>,
    },
}

#[derive(Clone)]
pub struct BleHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl BleHandle {
    pub fn send(&self, cmd: Command) {
        // Only fails if the actor thread has died, which is already logged.
        let _ = self.tx.send(cmd);
    }

    /// Send a command that carries a reply channel and wait for the answer.
    pub async fn request(&self, make: impl FnOnce(Reply) -> Command) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.send(make(tx));
        rx.await.map_err(|_| "Bluetooth service stopped".to_string())?
    }
}

pub fn start(shared: Arc<Shared>) -> BleHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("tug-ble".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("build BLE runtime");
                let local = tokio::task::LocalSet::new();
                local.block_on(&rt, actor::run(shared, rx));
                log::error!("BLE actor exited");
            })
            .expect("spawn BLE thread");
    }
    #[cfg(not(windows))]
    {
        // tug's Bluetooth backend is Windows-only; answer every request with an error.
        let _ = shared;
        let mut rx = rx;
        tauri::async_runtime::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                let reply = match cmd {
                    Command::PerformAction { reply, .. }
                    | Command::Media { reply, .. }
                    | Command::Pair { reply, .. }
                    | Command::RemovePairing { reply, .. }
                    | Command::UseDevice { reply, .. }
                    | Command::PairTexts { reply }
                    | Command::Forget { reply }
                    | Command::SetAdvertising { reply, .. } => reply,
                    Command::StartDiscovery | Command::StopDiscovery | Command::RescanDiscovery => continue,
                    Command::Inventory { reply } => {
                        let _ = reply.send(crate::bt_inventory::BtInventory::unavailable(
                            crate::bt_inventory::Trigger::OnDemand,
                            crate::state::now_ms(),
                            "Bluetooth is only supported on Windows",
                        ));
                        continue;
                    }
                };
                let _ = reply.send(Err("Bluetooth is only supported on Windows".into()));
            }
        });
    }
    BleHandle { tx }
}
