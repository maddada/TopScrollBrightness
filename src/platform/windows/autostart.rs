//! "Start with Windows": a value under `HKCU\...\Run`.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

use super::util::wide;
use crate::shared::settings::APP_NAME;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn is_enabled() -> bool {
    let (key, name) = (wide(RUN_KEY), wide(APP_NAME));
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    status == ERROR_SUCCESS
}

pub fn enable() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (key, name) = (wide(RUN_KEY), wide(APP_NAME));
    let data = wide(&format!("\"{}\"", exe.display()));
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS { Ok(()) } else { Err(format!("Couldn't write the startup entry (error {status}).")) }
}

pub fn disable() -> Result<(), String> {
    let (key, name) = (wide(RUN_KEY), wide(APP_NAME));
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    if status == ERROR_SUCCESS { Ok(()) } else { Err(format!("Couldn't remove the startup entry (error {status}).")) }
}

/// Keeps an existing entry pointing at this exe, in case it moved.
pub fn sync() {
    if is_enabled() {
        let _ = enable();
    }
}
