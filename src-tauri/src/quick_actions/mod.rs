//! Quick actions: the Tasks in tug's taskbar jump list (right-click tug's taskbar button or its
//! Start menu entry): New message, Tugboat, Copy latest code, Play a game and Settings.
//!
//! Each task launches `tug.exe --action <id>`. tug is single-instance, so when tug is already
//! running that second launch only hands its arguments to the running tug (lib.rs) and exits;
//! when it isn't, the new tug starts and the action waits for the window to be ready.
//! Either way the action lands in one pending queue here: the window is brought forward, the
//! UI is nudged with `quick-action`, and it takes what's waiting with `take_quick_actions` once
//! it has loaded. Taking drains the queue, so a click is carried out exactly once, however
//! early it came and however many nudges went out (or were lost to a reloading page).
//!
//! This module is the pure part (arguments, the task list, the app id, the queue); `win.rs` is
//! the thin COM part that hands the list to Windows.

#[cfg(windows)]
mod win;

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::state::events;

/// The command-line switch a task passes: `--action new-message` (or `--action=new-message`).
pub const ACTION_ARG: &str = "--action";
/// Settings › General › Show games: Play a game is only offered while it's on.
pub const SHOW_GAMES_SETTING: &str = "ui.showGames";
/// Actions waiting for the UI beyond this are dropped (nobody clicks this fast on purpose).
const MAX_PENDING: usize = 8;

/// What a jump-list task asks tug to do. Serialized as its id (`"new-message"`), the same
/// string the command line carries; mirrored as `QuickAction` in `src/types/protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuickAction {
    NewMessage,
    Tugboat,
    CopyCode,
    Game,
    Settings,
    /// Not a jump-list task: the quick text shortcut (`quick_text`) queues it, so a press is
    /// carried out once, even before the window has loaded. Not in `ALL`, so no task offers it
    /// and no `--action` launches it.
    QuickText,
}

impl QuickAction {
    /// The jump list's tasks, in the order it shows them.
    pub const ALL: [QuickAction; 5] = [
        QuickAction::NewMessage,
        QuickAction::Tugboat,
        QuickAction::CopyCode,
        QuickAction::Game,
        QuickAction::Settings,
    ];

    pub fn id(self) -> &'static str {
        match self {
            QuickAction::NewMessage => "new-message",
            QuickAction::Tugboat => "tugboat",
            QuickAction::CopyCode => "copy-code",
            QuickAction::Game => "game",
            QuickAction::Settings => "settings",
            QuickAction::QuickText => "quick-text",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The task's name in the jump list.
    pub fn title(self) -> &'static str {
        match self {
            QuickAction::NewMessage => "New message",
            QuickAction::Tugboat => "Tugboat",
            QuickAction::CopyCode => "Copy latest code",
            QuickAction::Game => "Play a game",
            QuickAction::Settings => "Settings",
            QuickAction::QuickText => "Text someone",
        }
    }

    /// The tooltip Windows shows when hovering the task.
    pub fn description(self) -> &'static str {
        match self {
            QuickAction::NewMessage => "Start a new text from your iPhone",
            QuickAction::Tugboat => "Send files and text to and from your phone",
            QuickAction::CopyCode => "Copy the newest verification code from your iPhone",
            QuickAction::Game => "Tugboat Run or Harbor Stack",
            QuickAction::Settings => "Open tug's settings",
            QuickAction::QuickText => "Send a text from your iPhone without leaving what you're doing",
        }
    }
}

/// The action a launch asks for, if any. `args` is the whole command line (the program path
/// first is fine). An unknown action is ignored rather than guessed at: tug just comes forward.
/// The quick action this launch should carry out: the one on its command line, unless this launch
/// is tug coming back from an update restart. The installer starts tug again with the arguments
/// the old process was started with, so a jump-list task clicked before the update (say "Play a
/// game") would otherwise run again by itself afterwards.
pub fn launch_action<I, S>(args: I, resuming_from_update: bool) -> Option<QuickAction>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if resuming_from_update {
        return None;
    }
    parse_args(args)
}

pub fn parse_args<I, S>(args: I) -> Option<QuickAction>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        let value = if arg == ACTION_ARG {
            args.next()?.as_ref().to_owned()
        } else if let Some(v) = arg.strip_prefix(ACTION_ARG).and_then(|r| r.strip_prefix('=')) {
            v.to_owned()
        } else {
            continue;
        };
        let action = QuickAction::from_id(value.trim());
        if action.is_none() {
            log::info!("quick actions: ignored an unknown action");
        }
        return action;
    }
    None
}

/// One jump-list task: what it's called and what it passes to tug.exe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub action: QuickAction,
    pub title: &'static str,
    pub description: &'static str,
    pub arguments: String,
}

/// The Tasks section. Play a game only while Settings › General › Show games is on, so
/// turning games off takes them out of the taskbar too.
pub fn tasks(show_games: bool) -> Vec<Task> {
    QuickAction::ALL
        .into_iter()
        .filter(|a| show_games || *a != QuickAction::Game)
        .map(|action| Task {
            action,
            title: action.title(),
            description: action.description(),
            arguments: format!("{ACTION_ARG} {}", action.id()),
        })
        .collect()
}

/// Whether Show games is on, from its stored value (on unless switched off).
pub fn show_games(stored: Option<&str>) -> bool {
    stored != Some("false")
}

/// The AppUserModelID tug's taskbar button and jump list share. The installer stamps the bundle
/// identifier on tug's Start menu shortcut, and the process claims the same id, so a pinned tug,
/// the Start menu entry and the running window are one taskbar button with one jump list,
/// however tug was started (shortcut, Start with Windows, or a portable copy run directly).
/// A build run from cargo's `target\debug` or `target\release` gets its own id, so trying a dev
/// build never repoints the installed tug's tasks at the dev exe.
pub fn app_id(identifier: &str, exe: &Path) -> String {
    if is_build_output(exe) {
        format!("{identifier}.dev")
    } else {
        identifier.to_owned()
    }
}

/// `…\target\debug\tug.exe`, `…\target-x\release\tug.exe`: an exe cargo just built.
fn is_build_output(exe: &Path) -> bool {
    let Some(dir) = exe.parent() else { return false };
    let profile = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let target = dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    matches!(profile.to_ascii_lowercase().as_str(), "debug" | "release")
        && target.to_ascii_lowercase().starts_with("target")
}

/// The exe path as a shell link target. tauri's `current_exe` is canonicalized, which on Windows
/// gives a verbatim path (`\\?\C:\Users\…\tug.exe`), and IShellLinkW::SetPath rejects those
/// with E_INVALIDARG, so the whole jump list failed. `\\?\C:\…` becomes `C:\…` and
/// `\\?\UNC\server\share\…` becomes `\\server\share\…`; anything else is passed on unchanged.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn shell_path(exe: &Path) -> std::path::PathBuf {
    let Some(text) = exe.to_str() else {
        return exe.to_path_buf();
    };
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}").into();
    }
    match text.strip_prefix(r"\\?\") {
        Some(drive) if drive.as_bytes().get(1) == Some(&b':') => drive.into(),
        _ => exe.to_path_buf(),
    }
}

/// Actions waiting for the UI to take them. A click on a task that's already waiting isn't
/// queued twice (pressing New message twice while tug starts opens it once).
#[derive(Debug, Default)]
pub struct Pending {
    queue: Vec<QuickAction>,
}

impl Pending {
    pub const fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Queue an action; false if it was already waiting or the queue is full.
    pub fn push(&mut self, action: QuickAction) -> bool {
        if self.queue.contains(&action) || self.queue.len() >= MAX_PENDING {
            return false;
        }
        self.queue.push(action);
        true
    }

    /// Everything waiting, oldest first, leaving the queue empty.
    pub fn take(&mut self) -> Vec<QuickAction> {
        std::mem::take(&mut self.queue)
    }
}

static PENDING: Mutex<Pending> = Mutex::new(Pending::new());
/// The id from `init`, kept for rebuilding the list when Show games changes.
static APP_ID: OnceLock<String> = OnceLock::new();
/// One rebuild at a time (a quick off-on of Show games starts two).
static BUILDING: Mutex<()> = Mutex::new(());

/// Queue an action for the UI (it takes it once it's ready).
pub fn queue(action: QuickAction) {
    let queued = PENDING.lock().unwrap_or_else(|e| e.into_inner()).push(action);
    log::info!(
        "quick action {}{}",
        action.id(),
        if queued { "" } else { " (already waiting)" }
    );
}

/// A task was clicked while tug is running: bring the window forward, queue the action and
/// nudge the UI to take it.
pub fn receive<R: Runtime>(app: &AppHandle<R>, action: QuickAction) {
    queue(action);
    crate::tray::show(app);
    let _ = app.emit(events::QUICK_ACTION, ());
}

/// What's waiting, for the UI (`take_quick_actions`).
pub fn take() -> Vec<QuickAction> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Claim tug's AppUserModelID for this process. Must run before any window is created, so the
/// taskbar button carries it from the start. A failure is logged and tug carries on (the jump
/// list may then show only on a pinned or Start menu tug).
pub fn init(identifier: &str) {
    let exe = tauri::utils::platform::current_exe().unwrap_or_default();
    let id = APP_ID.get_or_init(|| app_id(identifier, &exe));
    #[cfg(windows)]
    if let Err(e) = win::set_process_app_id(id) {
        log::warn!("quick actions: couldn't set the taskbar id: {e}");
    }
    #[cfg(not(windows))]
    let _ = id;
}

/// Hand the Tasks to Windows, off the calling thread (COM, and nothing should wait on the shell).
/// Called at startup and when Show games changes; failures are logged, never shown.
pub fn install(show_games: bool) {
    let Some(id) = APP_ID.get().cloned() else {
        return;
    };
    let exe = match tauri::utils::platform::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            log::warn!("quick actions: no exe path for the jump list: {e}");
            return;
        }
    };
    let tasks = tasks(show_games);
    let spawned = std::thread::Builder::new().name("tug-jump-list".into()).spawn(move || {
        let _one_at_a_time = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(windows)]
        match win::commit(&id, &exe, &tasks) {
            Ok(()) => log::info!("quick actions: jump list ready ({} tasks)", tasks.len()),
            Err(e) => log::warn!("quick actions: couldn't set up the jump list: {e}"),
        }
        #[cfg(not(windows))]
        let _ = (id, exe, tasks);
    });
    if let Err(e) = spawned {
        log::warn!("quick actions: couldn't start the jump list thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_action_from_a_launch() {
        let exe = r"C:\Program Files\tug\tug.exe";
        assert_eq!(
            parse_args([exe, "--action", "new-message"]),
            Some(QuickAction::NewMessage)
        );
        assert_eq!(parse_args([exe, "--action=copy-code"]), Some(QuickAction::CopyCode));
        assert_eq!(parse_args(["--action", "tugboat"]), Some(QuickAction::Tugboat));
        assert_eq!(parse_args([exe, "--action", "game"]), Some(QuickAction::Game));
        assert_eq!(parse_args([exe, "--action", "settings"]), Some(QuickAction::Settings));
        // Other switches around it don't matter.
        assert_eq!(
            parse_args([exe, "--minimized", "--action", "settings"]),
            Some(QuickAction::Settings)
        );
    }

    #[test]
    fn a_plain_or_odd_launch_asks_for_nothing() {
        let exe = r"C:\tug\tug.exe";
        assert_eq!(parse_args([exe]), None);
        assert_eq!(parse_args([exe, "--minimized"]), None);
        assert_eq!(parse_args::<[&str; 0], &str>([]), None);
        // An action switch with nothing after it, or something tug doesn't know.
        assert_eq!(parse_args([exe, "--action"]), None);
        assert_eq!(parse_args([exe, "--action", "format-c"]), None);
        assert_eq!(parse_args([exe, "--action="]), None);
        assert_eq!(parse_args([exe, "--action", "New-Message"]), None);
        // Only the exact switch: not a longer one that starts the same way.
        assert_eq!(parse_args([exe, "--actions", "settings"]), None);
        assert_eq!(parse_args([exe, "--actionx=settings"]), None);
    }

    #[test]
    fn the_first_action_wins() {
        assert_eq!(
            parse_args(["tug.exe", "--action", "game", "--action", "settings"]),
            Some(QuickAction::Game)
        );
    }

    #[test]
    fn every_task_launches_tug_with_an_action_tug_reads_back() {
        let all = tasks(true);
        assert_eq!(
            all.iter().map(|t| t.title).collect::<Vec<_>>(),
            ["New message", "Tugboat", "Copy latest code", "Play a game", "Settings"]
        );
        for task in &all {
            let args: Vec<&str> = task.arguments.split(' ').collect();
            assert_eq!(parse_args(args), Some(task.action), "{}", task.arguments);
            assert!(!task.description.is_empty());
        }
    }

    #[test]
    fn games_off_leaves_the_game_out() {
        let titles: Vec<_> = tasks(false).iter().map(|t| t.action).collect();
        assert_eq!(
            titles,
            [
                QuickAction::NewMessage,
                QuickAction::Tugboat,
                QuickAction::CopyCode,
                QuickAction::Settings
            ]
        );
        assert!(show_games(None));
        assert!(show_games(Some("true")));
        assert!(!show_games(Some("false")));
    }

    #[test]
    fn ids_round_trip_and_match_what_the_ui_gets() {
        for a in QuickAction::ALL {
            assert_eq!(QuickAction::from_id(a.id()), Some(a));
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(a.id()));
        }
    }

    #[test]
    fn quick_text_reaches_the_ui_but_is_never_a_task_or_a_launch() {
        assert_eq!(
            serde_json::to_value(QuickAction::QuickText).unwrap(),
            serde_json::json!("quick-text")
        );
        assert!(!QuickAction::ALL.contains(&QuickAction::QuickText));
        assert!(tasks(true).iter().all(|t| t.action != QuickAction::QuickText));
        assert_eq!(parse_args(["tug.exe", "--action", "quick-text"]), None);
    }

    #[test]
    fn installed_and_portable_use_the_identifier_and_a_cargo_build_its_own() {
        let id = "dev.example.tug";
        assert_eq!(app_id(id, Path::new(r"C:\Users\Sam\AppData\Local\tug\tug.exe")), id);
        assert_eq!(app_id(id, Path::new(r"D:\Apps\tug-portable\tug.exe")), id);
        // A folder that only happens to be called "release" isn't a cargo build.
        assert_eq!(app_id(id, Path::new(r"D:\Downloads\release\tug.exe")), id);
        assert_eq!(
            app_id(id, Path::new(r"C:\src\tug\src-tauri\target\debug\tug.exe")),
            "dev.example.tug.dev"
        );
        assert_eq!(
            app_id(id, Path::new(r"C:\src\tug\src-tauri\target-ci\release\tug.exe")),
            "dev.example.tug.dev"
        );
        assert_eq!(app_id(id, Path::new("")), id);
    }

    #[test]
    fn a_verbatim_exe_path_becomes_one_a_shell_link_accepts() {
        let plain = |p: &str| shell_path(Path::new(p)).to_str().unwrap().to_owned();
        // What tauri's canonicalized current_exe gives for the per-user install.
        assert_eq!(
            plain(r"\\?\C:\Users\Sam\AppData\Local\tug\tug.exe"),
            r"C:\Users\Sam\AppData\Local\tug\tug.exe"
        );
        assert_eq!(plain(r"\\?\UNC\nas\apps\tug\tug.exe"), r"\\nas\apps\tug\tug.exe");
        // Already plain: unchanged.
        assert_eq!(plain(r"C:\tug\tug.exe"), r"C:\tug\tug.exe");
        assert_eq!(plain(r"\\nas\apps\tug.exe"), r"\\nas\apps\tug.exe");
        // A verbatim path with no drive letter has no plain form: left as it is.
        assert_eq!(plain(r"\\?\Volume{0b1c}\tug\tug.exe"), r"\\?\Volume{0b1c}\tug\tug.exe");
        assert_eq!(plain(""), "");
    }

    #[test]
    fn a_click_is_taken_once_in_order_and_never_doubled_while_waiting() {
        let mut p = Pending::new();
        assert!(p.take().is_empty());
        assert!(p.push(QuickAction::CopyCode));
        assert!(p.push(QuickAction::NewMessage));
        // The same task again before the UI took the first: still one.
        assert!(!p.push(QuickAction::CopyCode));
        assert_eq!(p.take(), [QuickAction::CopyCode, QuickAction::NewMessage]);
        // Taken means gone: a second take (another nudge) runs nothing again.
        assert!(p.take().is_empty());
        // After it was taken, the next click counts again.
        assert!(p.push(QuickAction::CopyCode));
        assert_eq!(p.take(), [QuickAction::CopyCode]);
    }

    #[test]
    fn the_queue_is_bounded() {
        let mut p = Pending::new();
        // Only five distinct actions exist, so it can't overflow in practice; the cap still holds.
        for a in QuickAction::ALL {
            assert!(p.push(a));
        }
        assert_eq!(p.take().len(), QuickAction::ALL.len());
        p.queue = vec![QuickAction::Game; MAX_PENDING];
        assert!(!p.push(QuickAction::Settings));
    }

    #[test]
    fn an_update_restart_never_replays_the_old_launchs_action() {
        let args = ["tug.exe", "--action", "settings"];
        assert_eq!(launch_action(args, false), Some(QuickAction::Settings));
        assert_eq!(launch_action(args, true), None);
        assert_eq!(launch_action(["tug.exe"], false), None);
    }
}
