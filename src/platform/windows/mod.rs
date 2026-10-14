//! Windows backend.

mod autostart;
mod brightness;
mod hook;
mod instance;
mod osd;
mod tray;
mod util;

use std::time::Duration;

use instance::InstanceLock;

/// Launch flag: take over from a copy that's already running (e.g. after an update).
const REPLACE_ARG: &str = "--replace";

pub fn run() {
    let replace = std::env::args().any(|a| a == REPLACE_ARG);
    let lock = InstanceLock::acquire(Duration::ZERO).or_else(|| {
        replace.then(|| {
            instance::ask_existing_to_quit();
            InstanceLock::acquire(Duration::from_secs(5))
        })?
    });
    let Some(lock) = lock else {
        instance::signal_existing();
        return;
    };

    autostart::sync();
    let brightness = brightness::spawn();
    hook::start(brightness.clone());
    tray::run(lock, brightness);
}
