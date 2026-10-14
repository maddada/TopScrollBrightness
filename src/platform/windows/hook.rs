//! Low-level mouse hook: wheel events with the pointer on a screen's top edge
//! change that monitor's brightness instead of scrolling.
//!
//! Injected input counts too (mouse-sharing tools like Deskflow inject every
//! scroll), except MiddleClickDragScroll's own synthetic scrolling.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_MOUSEWHEEL,
};

use super::brightness::Brightness;
use crate::shared::level::{on_top_edge, wheel_percent};
use crate::shared::{log, settings};

/// How many pixels from the top of a screen count as its edge.
const EDGE_PX: i32 = 2;

/// Tag on the wheel events MiddleClickDragScroll injects.
const DRAG_SCROLL_MARKER: usize = 0x4D43_4453;

static BRIGHTNESS: OnceLock<Brightness> = OnceLock::new();

/// Installs the hook on a dedicated high-priority thread.
pub fn start(brightness: Brightness) {
    let _ = BRIGHTNESS.set(brightness);
    std::thread::Builder::new()
        .name("mouse-hook".into())
        .spawn(|| unsafe {
            // Windows silently drops low-level hooks that respond too slowly, so
            // this thread does nothing else and runs at the highest priority.
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                log::write("SetWindowsHookExW(WH_MOUSE_LL) failed");
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
            UnhookWindowsHookEx(hook);
        })
        .expect("failed to spawn mouse hook thread");
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam as u32 == WM_MOUSEWHEEL {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if take_scroll(info) {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Turns the scroll into a brightness change if the pointer is on a top edge.
fn take_scroll(info: &MSLLHOOKSTRUCT) -> bool {
    let s = settings::current();
    if !s.enabled || info.dwExtraInfo == DRAG_SCROLL_MARKER {
        return false;
    }
    let Some(brightness) = BRIGHTNESS.get() else { return false };
    let Some(monitor) = top_edge_monitor(info.pt) else { return false };
    if !brightness.handles(monitor) {
        return false;
    }
    let delta = (info.mouseData >> 16) as u16 as i16 as i32;
    brightness.nudge(monitor, wheel_percent(delta, s.step));
    true
}

/// The monitor whose top edge the pointer is on, if any.
fn top_edge_monitor(pt: POINT) -> Option<isize> {
    unsafe {
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return None;
        }
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let top = info.rcMonitor.top;
        let above = !MonitorFromPoint(POINT { x: pt.x, y: top - 1 }, MONITOR_DEFAULTTONULL).is_null();
        on_top_edge(pt.y, top, EDGE_PX, above).then_some(monitor as isize)
    }
}
