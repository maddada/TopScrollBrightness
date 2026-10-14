//! One backend per OS. Each provides `run()`: install the input hook, talk to
//! the monitors, show the tray icon and run until the user quits.
//!
//! A macOS port would use a CGEventTap for the scroll wheel and DDC over
//! IOAVService (as MonitorControl does); Linux would use ddcutil/i2c-dev.
//! Both reuse `shared::level` and `shared::hud`.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::run;

#[cfg(not(windows))]
pub fn run() {
    eprintln!("{} only supports Windows for now.", crate::shared::settings::APP_NAME);
}
