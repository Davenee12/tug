//! Starting up on a stranger's PC: say why tug can't start instead of vanishing, offer to set a
//! database that won't open aside, log panics before the (release-build) abort, and fit the window
//! on small or high-DPI screens.

use std::path::{Path, PathBuf};

/// Where tug's logs go: `%LOCALAPPDATA%\<identifier>\logs` (tauri-plugin-log's default folder,
/// the same as `app_log_dir()`), worked out without an `AppHandle` so it can be named even when
/// the app never got that far.
pub fn log_dir(identifier: &str) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join(identifier).join("logs"))
}

/// Log every panic (thread, location, message) to tug.log and flush before the release build's
/// `panic = "abort"` ends the process, so a crash is never a mystery. A release build also says so
/// on screen, since the window is about to disappear.
pub fn install_panic_hook(log_dir: Option<PathBuf>) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        log::error!("panic on thread '{name}': {info}");
        log::logger().flush();
        if cfg!(not(debug_assertions)) {
            show_error(&format!(
                "tug ran into a problem and has to close.\n\nWhat happened is in the log (tug.log) in:\n{}\n\n\
                 Start tug again from the Start menu.",
                where_logs(log_dir.as_deref())
            ));
        }
        default(info);
    }));
}

fn where_logs(dir: Option<&Path>) -> String {
    dir.map(|d| d.display().to_string())
        .unwrap_or_else(|| "tug's logs folder".into())
}

/// Tell the user tug couldn't start at all (a setup or WebView failure) and where the log is.
pub fn show_fatal(error: &str, log_dir: Option<&Path>) {
    show_error(&format!(
        "tug couldn't start.\n\n{error}\n\nMore detail is in the log (tug.log) in:\n{}",
        where_logs(log_dir)
    ));
}

/// The database didn't open (corrupt, locked by a sync app, read-only folder, failed upgrade).
/// Ask whether to start fresh; true means set it aside and try again, false means quit.
pub fn ask_start_fresh(db: &Path, error: &str, log_dir: Option<&Path>) -> bool {
    let text = format!(
        "tug couldn't open its database, so it can't start.\n\n{error}\n\nDatabase: {}\nLog (tug.log): {}\n\n\
         Choose Yes to start fresh: tug renames the database (nothing is deleted) and starts with an empty \
         one, so your history and settings start over.\n\
         Choose No to quit, for example to close a sync app that may be holding the file and try again.",
        db.display(),
        where_logs(log_dir),
    );
    ask_yes_no("tug couldn't start", &text)
}

/// The name a broken database is set aside under: `tug.db.broken-20261007-142501` (UTC).
pub fn broken_name(db: &Path, unix_secs: u64) -> PathBuf {
    let mut name = db
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "tug.db".into());
    name.push(format!(".broken-{}", stamp(unix_secs)));
    db.with_file_name(name)
}

/// `YYYYMMDD-HHMMSS` for a Unix time (UTC), no date crate needed.
fn stamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day), Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Rename `tug.db` (and its `-wal`/`-shm` companions, which belong to it) to the broken name.
/// Returns where the database went.
pub fn set_aside(db: &Path) -> std::io::Result<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let to = broken_name(db, secs);
    if db.exists() {
        std::fs::rename(db, &to)?;
    }
    for suffix in ["-wal", "-shm"] {
        let mut side = db.as_os_str().to_os_string();
        side.push(suffix);
        let side = PathBuf::from(side);
        if side.exists() {
            let mut dest = to.as_os_str().to_os_string();
            dest.push(suffix);
            // A leftover journal can't be applied to a fresh database; move it with its file.
            let _ = std::fs::rename(&side, PathBuf::from(dest));
        }
    }
    Ok(to)
}

#[cfg(windows)]
fn message_box(title: &str, text: &str, yes_no: bool) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };
    let style = if yes_no { MB_YESNO } else { MB_OK } | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST;
    // SAFETY: plain FFI with NUL-terminated strings that outlive the call.
    let r = unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), style) };
    r == IDYES
}

#[cfg(not(windows))]
fn message_box(_title: &str, text: &str, _yes_no: bool) -> bool {
    eprintln!("{text}");
    false
}

fn show_error(text: &str) {
    message_box("tug", text, false);
}

fn ask_yes_no(title: &str, text: &str) -> bool {
    message_box(title, text, true)
}

// ---- Fitting the window on small screens ------------------------------------------------------

/// Room the title bar and borders take around the window's inner (web) area, in logical pixels.
const FRAME_WIDTH: f64 = 16.0;
const FRAME_HEIGHT: f64 = 48.0;

/// A window size and minimum size (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub width: f64,
    pub height: f64,
    pub min_width: f64,
    pub min_height: f64,
}

/// The configured size (and minimum) shrunk to fit a monitor's work area (logical pixels, the
/// screen minus the taskbar), frame included. None when it already fits.
pub fn fit_to_work_area(size: (f64, f64), min: (f64, f64), work: (f64, f64)) -> Option<Fit> {
    let max_w = (work.0 - FRAME_WIDTH).max(1.0);
    let max_h = (work.1 - FRAME_HEIGHT).max(1.0);
    if size.0 <= max_w && size.1 <= max_h {
        return None;
    }
    let width = size.0.min(max_w);
    let height = size.1.min(max_h);
    Some(Fit {
        width,
        height,
        min_width: min.0.min(width),
        min_height: min.1.min(height),
    })
}

/// Shrink the (still hidden) main window to fit the screen it opens on, then center it. A 1280×820
/// window is taller than a 1366×768 laptop's screen, and wider than a small screen at 150%.
pub fn fit_window(window: &tauri::WebviewWindow) {
    use tauri::{LogicalSize, Size};
    let monitor = match window.current_monitor() {
        Ok(Some(m)) => m,
        _ => match window.primary_monitor() {
            Ok(Some(m)) => m,
            _ => return,
        },
    };
    let scale = monitor.scale_factor();
    let work = monitor.work_area().size.to_logical::<f64>(scale);
    let Ok(inner) = window.inner_size() else { return };
    let inner = inner.to_logical::<f64>(window.scale_factor().unwrap_or(scale));
    // The configured minimum (tauri.conf.json); the window API can't read it back.
    const MIN: (f64, f64) = (960.0, 600.0);
    if let Some(fit) = fit_to_work_area((inner.width, inner.height), MIN, (work.width, work.height)) {
        log::info!(
            "window: fitting {}x{} into a {}x{} work area",
            inner.width,
            inner.height,
            work.width,
            work.height
        );
        let _ = window.set_min_size(Some(Size::Logical(LogicalSize::new(fit.min_width, fit.min_height))));
        let _ = window.set_size(Size::Logical(LogicalSize::new(fit.width, fit.height)));
    }
    let _ = window.center();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_name_stamps_utc_time() {
        let db = Path::new(r"C:\data\tug.db");
        // 2026-10-07 14:25:01 UTC.
        assert_eq!(
            broken_name(db, 1_791_383_101),
            PathBuf::from(r"C:\data\tug.db.broken-20261007-142501")
        );
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn set_aside_moves_the_database_and_its_journal() {
        let dir = std::env::temp_dir().join(format!("tug-startup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("tug.db");
        std::fs::write(&db, b"not a database").unwrap();
        std::fs::write(dir.join("tug.db-wal"), b"wal").unwrap();
        let to = set_aside(&db).unwrap();
        assert!(!db.exists() && to.exists());
        assert!(!dir.join("tug.db-wal").exists());
        let mut wal = to.as_os_str().to_os_string();
        wal.push("-wal");
        assert!(PathBuf::from(wal).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn window_fits_small_and_scaled_screens() {
        // Fits a 1920x1040 work area: left alone.
        assert_eq!(
            fit_to_work_area((1280.0, 820.0), (960.0, 600.0), (1920.0, 1040.0)),
            None
        );
        // 1366x768 laptop (728 tall without the taskbar): shorter, same width.
        let f = fit_to_work_area((1280.0, 820.0), (960.0, 600.0), (1366.0, 728.0)).unwrap();
        assert_eq!((f.width, f.height), (1280.0, 680.0));
        assert_eq!((f.min_width, f.min_height), (960.0, 600.0));
        // 1280x720 at 150% (853x440 logical work area): smaller than the minimum, which follows.
        let f = fit_to_work_area((1280.0, 820.0), (960.0, 600.0), (853.0, 440.0)).unwrap();
        assert_eq!((f.width, f.height), (837.0, 392.0));
        assert_eq!((f.min_width, f.min_height), (837.0, 392.0));
    }
}
