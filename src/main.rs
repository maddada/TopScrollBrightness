//! TopScrollBrightness: put the pointer at the top edge of a screen and scroll
//! to change that monitor's brightness (its own hardware setting, over DDC/CI).
//!
//! Layout:
//! - `shared/`   OS-independent: brightness maths, indicator artwork, settings, icon, log
//! - `platform/` one backend per OS: mouse hook, monitor control, indicator window, tray

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;
mod shared;

fn main() {
    shared::log::install_panic_hook();
    shared::settings::load();
    platform::run();
}
