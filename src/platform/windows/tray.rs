//! Notification-area icon, its menu, and the hidden window that owns them.
//! Runs the main thread's message loop (which also drives the indicator).

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, ChangeWindowMessageFilterEx, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HICON, KillTimer,
    MB_ICONWARNING, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MFT_RADIOCHECK, MSG, MSGFLT_ALLOW, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow,
    SetTimer, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_DISPLAYCHANGE, WM_NULL, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};

use super::brightness::Brightness;
use super::instance::InstanceLock;
use super::util::{copy_wide, wide};
use super::{autostart, osd};
use crate::shared::{icon, settings};

pub const CLASS_NAME: &str = "TopScrollBrightness.Tray";

const WM_TRAY: u32 = WM_APP + 1;
pub const WM_SHOW_TIP: u32 = WM_APP + 2;
pub const WM_QUIT_APP: u32 = WM_APP + 3;

const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const RETRY_TIMER: usize = 1;

const ID_ENABLED: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_QUIT: usize = 3;
/// Step menu items are this plus the step in percent.
const ID_STEP_BASE: usize = 100;

const TIP: &str = "Move the pointer to the very top of a screen and scroll to change that monitor's brightness. \
                   Click this icon to turn it on or off.";

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct Tray {
    // Held for its lifetime: releasing it lets another copy start.
    _instance: InstanceLock,
    brightness: Brightness,
    icon_on: HICON,
    icon_off: HICON,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Creates the tray icon and the indicator, and runs the message loop until the app quits.
pub fn run(instance: InstanceLock, brightness: Brightness) {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );

        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);
        for msg in [taskbar_created, WM_SHOW_TIP, WM_QUIT_APP] {
            ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, std::ptr::null_mut());
        }

        let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
        TRAY.set(Some(Tray {
            _instance: instance,
            brightness,
            icon_on: make_icon(size, true),
            icon_off: make_icon(size, false),
        }));
        osd::create();

        if !add_icon(hwnd) {
            // Explorer may not be ready yet right after sign-in.
            SetTimer(hwnd, RETRY_TIMER, 2000, None);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match lparam as u32 & 0xFFFF {
                NIN_SELECT | NIN_KEYSELECT => toggle_enabled(hwnd),
                WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_SHOW_TIP => {
            show_tip(hwnd, &format!("{} is already running", settings::APP_NAME));
            0
        }
        WM_QUIT_APP => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DISPLAYCHANGE => {
            TRAY.with_borrow(|t| t.as_ref().map(|t| t.brightness.displays_changed()));
            0
        }
        WM_TIMER if wparam == RETRY_TIMER => {
            if add_icon(hwnd) {
                unsafe { KillTimer(hwnd, RETRY_TIMER) };
            }
            0
        }
        WM_DESTROY => {
            let nid = notify_data(hwnd);
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid);
                PostQuitMessage(0);
            }
            0
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            add_icon(hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let enabled = settings::current().enabled;
    let icon = TRAY
        .with_borrow(|t| t.as_ref().map(|t| if enabled { t.icon_on } else { t.icon_off }))
        .unwrap_or(std::ptr::null_mut());
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    let state = if enabled { "on" } else { "off" };
    copy_wide(&mut nid.szTip, &format!("{}: {state}", settings::APP_NAME));
    nid
}

fn add_icon(hwnd: HWND) -> bool {
    let mut nid = notify_data(hwnd);
    unsafe {
        if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
            return false;
        }
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
    if !settings::current().welcomed {
        show_tip(hwnd, &format!("{} is running", settings::APP_NAME));
        settings::update(|s| s.welcomed = true);
    }
    true
}

fn show_tip(hwnd: HWND, title: &str) {
    let mut nid = notify_data(hwnd);
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, TIP);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn toggle_enabled(hwnd: HWND) {
    settings::update(|s| s.enabled = !s.enabled);
    let nid = notify_data(hwnd);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn show_menu(hwnd: HWND) {
    let s = settings::current();
    let autostart = autostart::is_enabled();

    let command = unsafe {
        let add = |menu, id: usize, text: &str, flags: u32| {
            let text = wide(text);
            AppendMenuW(menu, MF_STRING | flags, id, text.as_ptr());
        };
        let checked = |on: bool| if on { MF_CHECKED } else { 0 };

        let steps = CreatePopupMenu();
        for step in settings::STEPS {
            add(steps, ID_STEP_BASE + step as usize, &format!("{step}%"), MFT_RADIOCHECK | checked(s.step == step));
        }

        let menu = CreatePopupMenu();
        add(menu, ID_ENABLED, "Enabled", checked(s.enabled));
        let label = wide("Step per scroll notch");
        AppendMenuW(menu, MF_POPUP, steps as usize, label.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(menu, ID_AUTOSTART, "Start with Windows", checked(autostart));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(menu, ID_QUIT, "Quit", 0);

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Required so the menu closes when clicking elsewhere.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu); // also destroys the submenu
        command as usize
    };

    match command {
        ID_ENABLED => toggle_enabled(hwnd),
        ID_AUTOSTART => {
            let result = if autostart { autostart::disable() } else { autostart::enable() };
            if let Err(e) = result {
                let (text, caption) = (wide(&e), wide(settings::APP_NAME));
                unsafe { MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), MB_ICONWARNING) };
            }
        }
        ID_QUIT => unsafe {
            DestroyWindow(hwnd);
        },
        id if id > ID_STEP_BASE => settings::update(|s| s.step = (id - ID_STEP_BASE) as u32),
        _ => {}
    }
}

fn make_icon(size: u32, active: bool) -> HICON {
    let rgba = icon::render_rgba(size, active);
    let bgra: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    // The 32-bit colour data carries alpha, so the AND mask is all zeros.
    let mask = vec![0u8; (size.div_ceil(16) * 2 * size) as usize];
    unsafe {
        CreateIcon(
            GetModuleHandleW(std::ptr::null()),
            size as i32,
            size as i32,
            1,
            32,
            mask.as_ptr(),
            bgra.as_ptr(),
        )
    }
}
