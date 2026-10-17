//! The listening indicator: a small pill at the bottom of the screen, drawn with plain GDI (no
//! webview, so it costs nothing while hidden and appears instantly). It never takes focus or
//! clicks, so typing still goes where it was going.
//!
//! The window belongs to the voice shortcut thread (which pumps its messages); any thread can
//! show or hide it by posting a message.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, EndPaint,
    FillRect, GetDC, GetDeviceCaps, GetMonitorInfoW, GetTextExtentPoint32W, InvalidateRect, MonitorFromWindow,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HFONT, HGDIOBJ, LOGPIXELSY, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, KillTimer, PostMessageW, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_APP, WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// What the dot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Coral: the microphone is open.
    Listening,
    /// Amber: working on it.
    Working,
    /// Red: something needs the person.
    Problem,
}

const WM_SHOW: u32 = WM_APP + 20;
const WM_HIDE: u32 = WM_APP + 21;
const HIDE_TIMER: usize = 1;

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static CONTENT: Mutex<(String, Option<Look>)> = Mutex::new((String::new(), None));
static FONT: AtomicIsize = AtomicIsize::new(0);

// tug's dark surface and its accents (src/style.css), as GDI's 0x00BBGGRR.
const SURFACE: u32 = 0x0015_1718; // #181715
const ON_DARK: u32 = 0x00f5_f9fa; // #faf9f5
const CORAL: u32 = 0x005c_78cc; // #cc785c
const AMBER: u32 = 0x005a_a5e8; // #e8a55a
const ERROR: u32 = 0x0045_45c6; // #c64545

/// Create the (hidden) indicator window. Call on the thread that pumps messages.
pub fn create() {
    // SAFETY: registering a class and creating a window on this thread, with static strings.
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("tugVoiceIndicator"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            w!("tugVoiceIndicator"),
            w!("tug voice typing"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            None,
            None,
        );
        match hwnd {
            Ok(hwnd) => {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 240, LWA_ALPHA);
                WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
            }
            Err(e) => log::warn!("voice typing indicator unavailable: {e}"),
        }
    }
}

fn window() -> Option<HWND> {
    let h = WINDOW.load(Ordering::SeqCst);
    (h != 0).then_some(HWND(h as *mut _))
}

/// Show `text` with a dot; hide by itself after `hide_after_ms` (0: stay until hidden).
pub fn show(text: &str, look: Look, hide_after_ms: u32) {
    *CONTENT.lock().unwrap_or_else(|e| e.into_inner()) = (text.to_string(), Some(look));
    if let Some(hwnd) = window() {
        // SAFETY: posting a plain message to tug's own window.
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_SHOW, WPARAM(hide_after_ms as usize), LPARAM(0));
        }
    }
}

pub fn hide() {
    if let Some(hwnd) = window() {
        // SAFETY: as above.
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_HIDE, WPARAM(0), LPARAM(0));
        }
    }
}

fn font(scale: i32) -> HFONT {
    let f = FONT.load(Ordering::SeqCst);
    if f != 0 {
        return HFONT(f as *mut _);
    }
    // SAFETY: creating a GDI font with a static face name; kept for the app's lifetime.
    let f = unsafe {
        CreateFontW(
            -14 * scale / 96,
            0,
            0,
            0,
            500,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    };
    FONT.store(f.0 as isize, Ordering::SeqCst);
    f
}

fn text_wide() -> (Vec<u16>, Look) {
    let (text, look) = CONTENT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (text.encode_utf16().collect(), look.unwrap_or(Look::Listening))
}

/// Size the pill to its text and put it at the bottom centre of the screen in front.
///
/// SAFETY: call on the window's thread with its handle.
unsafe fn place(hwnd: HWND) {
    let (text, _) = text_wide();
    let dc = GetDC(Some(hwnd));
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY).max(96);
    let old = SelectObject(dc, HGDIOBJ(font(dpi).0));
    let mut size = Default::default();
    let _ = GetTextExtentPoint32W(dc, &text, &mut size);
    SelectObject(dc, old);
    ReleaseDC(Some(hwnd), dc);
    let s = |v: i32| v * dpi / 96;
    let height = s(40);
    let width = size.cx + s(20 + 10 + 10 + 20);
    let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        }
    };
    let x = work.left + (work.right - work.left - width) / 2;
    let y = work.bottom - height - s(56);
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, height, height);
    SetWindowRgn(hwnd, Some(region), false);
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

/// SAFETY: call from WM_PAINT on the window's thread.
unsafe fn paint(hwnd: HWND) {
    let (mut text, look) = text_wide();
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY).max(96);
    let s = |v: i32| v * dpi / 96;
    let rect = ps.rcPaint;
    let mut whole = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut whole);
    let bg = CreateSolidBrush(COLORREF(SURFACE));
    FillRect(dc, &rect, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));

    let dot_color = match look {
        Look::Listening => CORAL,
        Look::Working => AMBER,
        Look::Problem => ERROR,
    };
    let dot = CreateSolidBrush(COLORREF(dot_color));
    let old_brush = SelectObject(dc, HGDIOBJ(dot.0));
    let pen = windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_PEN);
    let old_pen = SelectObject(dc, pen);
    let mid = (whole.bottom - whole.top) / 2;
    let _ = Ellipse(dc, s(18), mid - s(5), s(28), mid + s(5));
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    let _ = DeleteObject(HGDIOBJ(dot.0));

    let old_font = SelectObject(dc, HGDIOBJ(font(dpi).0));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(ON_DARK));
    let mut text_rect = RECT {
        left: s(38),
        top: whole.top,
        right: whole.right - s(16),
        bottom: whole.bottom,
    };
    DrawTextW(dc, &mut text, &mut text_rect, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    SelectObject(dc, old_font);
    let _ = EndPaint(hwnd, &ps);
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the window procedure of tug's own window, on its thread.
    unsafe {
        match msg {
            WM_SHOW => {
                place(hwnd);
                let _ = InvalidateRect(Some(hwnd), None, true);
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                if wparam.0 > 0 {
                    SetTimer(Some(hwnd), HIDE_TIMER, wparam.0 as u32, None);
                }
                LRESULT(0)
            }
            WM_HIDE | WM_TIMER => {
                let _ = KillTimer(Some(hwnd), HIDE_TIMER);
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
