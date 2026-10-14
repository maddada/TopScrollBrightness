//! The on-screen indicator, styled like Windows 11's volume indicator: a
//! click-through, never-focused panel centred near the top of the monitor being
//! adjusted, just below where the volume indicator appears so both can be seen
//! at once. It fades out shortly after the last change.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

use windows_sys::Win32::Foundation::{COLORREF, ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DRAW_TEXT_FORMAT, DT_CENTER, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    FW_NORMAL, GdiFlush, GetMonitorInfoW, GetTextFaceW, HBITMAP, HDC, HFONT, HGDIOBJ, HMONITOR, MONITORINFO,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, KillTimer, PostMessageW, RegisterClassExW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetTimer, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WM_APP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::util::wide;
use crate::shared::hud::{Hud, Palette};
use crate::shared::settings::APP_NAME;

const WM_OSD_SHOW: u32 = WM_APP + 20;
const HOLD_TIMER: usize = 1;
const FADE_TIMER: usize = 2;
const HOLD_MS: u32 = 1500;
const FADE_STEP_MS: u32 = 16;
const FADE_STEPS: u8 = 10;
/// Gap between the top of the usable screen area (below any top bar) and
/// Windows' volume indicator, in 96-DPI pixels.
const VOLUME_TOP_GAP: f64 = 25.0;
/// Space between the volume indicator and this one below it, in 96-DPI pixels.
const STACK_GAP: f64 = 12.0;
/// The "Brightness" glyph in Windows' icon fonts.
const SUN_GLYPH: &str = "\u{E706}";
const ICON_FONTS: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];
const TEXT_FONTS: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];

static HWND_OSD: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ALPHA: AtomicU8 = AtomicU8::new(0);

/// Creates the indicator window (hidden). Call on the UI thread.
pub fn create() {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(&format!("{APP_NAME}.Indicator"));
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        HWND_OSD.store(hwnd, Ordering::Release);
    }
}

/// Shows `value` (at `fraction` of the range) on `monitor`. Callable from any thread.
pub fn show(monitor: isize, fraction: f64, value: u32) {
    let hwnd = HWND_OSD.load(Ordering::Acquire);
    if !hwnd.is_null() {
        let permille = (fraction.clamp(0.0, 1.0) * 1000.0).round() as usize;
        unsafe { PostMessageW(hwnd, WM_OSD_SHOW, (value as usize) << 16 | permille, monitor) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_OSD_SHOW => {
            let (value, fraction) = ((wparam >> 16) as u32, (wparam & 0xFFFF) as f64 / 1000.0);
            draw(hwnd, lparam as HMONITOR, fraction, value);
            unsafe {
                KillTimer(hwnd, FADE_TIMER);
                SetTimer(hwnd, HOLD_TIMER, HOLD_MS, None);
            }
            0
        }
        WM_TIMER if wparam == HOLD_TIMER => {
            unsafe {
                KillTimer(hwnd, HOLD_TIMER);
                SetTimer(hwnd, FADE_TIMER, FADE_STEP_MS, None);
            }
            0
        }
        WM_TIMER if wparam == FADE_TIMER => {
            let alpha = ALPHA.load(Ordering::Relaxed).saturating_sub(255 / FADE_STEPS + 1);
            set_alpha(hwnd, alpha);
            if alpha == 0 {
                unsafe {
                    KillTimer(hwnd, FADE_TIMER);
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

fn set_alpha(hwnd: HWND, alpha: u8) {
    ALPHA.store(alpha, Ordering::Relaxed);
    let blend = blend(alpha);
    unsafe {
        UpdateLayeredWindow(
            hwnd,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
            &blend,
            ULW_ALPHA,
        )
    };
}

/// Renders the indicator and shows it on `monitor`.
fn draw(hwnd: HWND, monitor: HMONITOR, fraction: f64, value: u32) {
    unsafe {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..std::mem::zeroed() };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        let (mut dpi, mut unused) = (96, 96);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let scale = dpi.max(96) as f64 / 96.0;
        let Some(surface) = Surface::render(scale, fraction, value, &theme_palette()) else { return };

        // Centred, just below where Windows shows its volume indicator (which is
        // the same size), so the two never overlap.
        let work = info.rcWork;
        let position = POINT {
            x: work.left + (work.right - work.left - surface.width) / 2,
            y: work.top + ((VOLUME_TOP_GAP + STACK_GAP) * scale).round() as i32 + surface.height,
        };
        let size = SIZE { cx: surface.width, cy: surface.height };
        let origin = POINT { x: 0, y: 0 };
        ALPHA.store(255, Ordering::Relaxed);
        let blend = blend(255);
        UpdateLayeredWindow(hwnd, std::ptr::null_mut(), &position, &size, surface.dc, &origin, 0, &blend, ULW_ALPHA);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Stay above other always-on-top windows that came up since.
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// The indicator's pixels in a GDI bitmap: premultiplied BGRA, as layered windows expect.
pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    pub(crate) bits: *mut u8,
    pub width: i32,
    pub height: i32,
}

impl Surface {
    pub fn render(scale: f64, fraction: f64, value: u32, palette: &Palette) -> Option<Self> {
        let hud = Hud::new(scale);
        let (width, height) = (hud.width as i32, hud.height as i32);
        let art = hud.render(fraction, palette);
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let header = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let bitmap_info = BITMAPINFO { bmiHeader: header, ..std::mem::zeroed() };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old_bitmap = SelectObject(dc, bitmap);
            let surface = Self { dc, bitmap, old_bitmap, bits: bits.cast(), width, height };

            // Opaque colour first, so the text anti-aliases against the background.
            let pixels = std::slice::from_raw_parts_mut(surface.bits, art.len());
            for (px, rgba) in pixels.chunks_exact_mut(4).zip(art.chunks_exact(4)) {
                px.copy_from_slice(&[rgba[2], rgba[1], rgba[0], 255]);
            }
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, colorref(palette.foreground));
            draw_text(dc, &ICON_FONTS, hud.icon_px(), SUN_GLYPH, hud.icon_box(), DT_CENTER);
            draw_text(dc, &TEXT_FONTS, hud.number_px(), &value.to_string(), hud.number_box(), DT_RIGHT);
            GdiFlush();
            // Then the panel's shape, premultiplied.
            for (px, rgba) in pixels.chunks_exact_mut(4).zip(art.chunks_exact(4)) {
                let a = rgba[3] as u32;
                for c in &mut px[..3] {
                    *c = (*c as u32 * a / 255) as u8;
                }
                px[3] = a as u8;
            }
            Some(surface)
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// Draws one line of text, vertically centred in `area`, with the first of
/// `faces` that's installed.
fn draw_text(dc: HDC, faces: &[&str], px: i32, text: &str, area: (i32, i32, i32, i32), align: DRAW_TEXT_FORMAT) {
    let (left, top, right, bottom) = area;
    let mut rect = RECT { left, top, right, bottom };
    let text = wide(text);
    unsafe {
        let font = installed_font(dc, faces, px);
        let old_font = SelectObject(dc, font);
        DrawTextW(dc, text.as_ptr(), -1, &mut rect, align | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(dc, old_font);
        DeleteObject(font);
    }
}

/// A font of `px` pixels (em size) in the first of `faces` that Windows has.
fn installed_font(dc: HDC, faces: &[&str], px: i32) -> HFONT {
    let create = |face: &str| unsafe {
        let name = wide(face);
        CreateFontW(
            -px,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            0,
            name.as_ptr(),
        )
    };
    for face in faces {
        let font = create(face);
        let mut actual = [0u16; 64];
        let len = unsafe {
            let old = SelectObject(dc, font);
            let len = GetTextFaceW(dc, actual.len() as i32, actual.as_mut_ptr());
            SelectObject(dc, old);
            len
        };
        // GetTextFaceW counts the terminating NUL.
        let actual = String::from_utf16_lossy(&actual[..(len.max(1) - 1) as usize]);
        if actual.eq_ignore_ascii_case(face) {
            return font;
        }
        unsafe { DeleteObject(font) };
    }
    create(faces[faces.len() - 1])
}

/// Colours for the current Windows theme and accent colour.
fn theme_palette() -> Palette {
    let light = read_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "SystemUsesLightTheme")
        .is_some_and(|v| v != 0);
    // Windows' accent palette: eight RGBA shades from lightest to darkest. Controls
    // use the second shade on dark backgrounds and the fifth on light ones.
    let mut palette = [0u8; 32];
    let found = read_binary(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent", "AccentPalette", &mut palette);
    let shade = |i: usize| [palette[i * 4], palette[i * 4 + 1], palette[i * 4 + 2]];
    match (light, found) {
        (false, true) => Palette::dark(shade(1)),
        (true, true) => Palette::light(shade(4)),
        (false, false) => Palette::dark([76, 194, 255]),
        (true, false) => Palette::light([0, 95, 184]),
    }
}

fn read_dword(key: &str, name: &str) -> Option<u32> {
    let (key, name) = (wide(key), wide(name));
    let (mut value, mut size) = (0u32, 4u32);
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(value)
}

fn read_binary(key: &str, name: &str, out: &mut [u8]) -> bool {
    let (key, name) = (wide(key), wide(name));
    let mut size = out.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            out.as_mut_ptr().cast(),
            &mut size,
        )
    };
    status == ERROR_SUCCESS && size as usize == out.len()
}

fn colorref([r, g, b]: [u8; 3]) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}
