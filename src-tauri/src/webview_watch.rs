//! Logging WebView2 process-failed events. tug had a crash on 2026-10-05 where the log filled with
//! "WebView2 error ... 0x8007139F" as contacts and photos synced, with nothing from the web side
//! captured. Tauri doesn't surface a crash event of its own, but it does hand back the raw WebView2
//! controller via `with_webview`; from there we can register `ProcessFailed` and log it, so the next
//! such crash says which process failed rather than only showing the fallout.

/// Register a WebView2 `ProcessFailed` handler that logs at warn. Best-effort: any failure to reach
/// the controller is logged once and otherwise ignored — crash logging is never a reason not to run.
#[cfg(windows)]
pub fn watch(window: &tauri::WebviewWindow) {
    let result = window.with_webview(|webview| {
        if let Err(e) = install(&webview) {
            log::warn!("WebView2 crash logging unavailable: {}", e.message());
        }
    });
    if let Err(e) = result {
        log::warn!("WebView2 crash logging unavailable: {e}");
    }
}

#[cfg(windows)]
fn install(webview: &tauri::webview::PlatformWebview) -> windows::core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2ProcessFailedEventArgs, COREWEBVIEW2_PROCESS_FAILED_KIND,
    };
    use webview2_com::ProcessFailedEventHandler;

    // SAFETY: the controller and core WebView2 come straight from Tauri's live webview, and the
    // handler only reads the event args (through the COM out-param getter) and logs; nothing is
    // stored across the call.
    unsafe {
        let controller = webview.controller();
        let core: ICoreWebView2 = controller.CoreWebView2()?;
        let handler = ProcessFailedEventHandler::create(Box::new(
            |_sender: Option<ICoreWebView2>, args: Option<ICoreWebView2ProcessFailedEventArgs>| {
                // The kind is an i32 enum (browser process exited, render process exited/unresponsive,
                // …); the value is enough to tell a hang from a crash in the log.
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(-1);
                if let Some(a) = args.as_ref() {
                    let _ = a.ProcessFailedKind(&mut kind);
                }
                log::warn!(
                    "WebView2 process failed (kind {}): the window's web content crashed or hung",
                    kind.0
                );
                Ok(())
            },
        ));
        // The registration token is only needed to unsubscribe, which we never do (the handler lives
        // as long as the window); keep it on the stack.
        let mut token: i64 = 0;
        core.add_ProcessFailed(&handler, &mut token)?;
    }
    Ok(())
}

/// Off Windows there's no WebView2 to watch; the call is a no-op so the setup path stays identical.
#[cfg(not(windows))]
pub fn watch(_window: &tauri::WebviewWindow) {}
