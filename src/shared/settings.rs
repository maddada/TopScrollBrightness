//! The few preferences the app remembers, in `settings.ini` (`key = value`)
//! inside the per-user config folder.

use std::path::PathBuf;
use std::sync::RwLock;

pub const APP_NAME: &str = "TopScrollBrightness";

/// Step sizes offered in the tray menu, in percent per wheel notch.
pub const STEPS: [u32; 4] = [1, 2, 5, 10];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// Brightness change per wheel notch, in percent of the monitor's range.
    pub step: u32,
    /// The first-run tip has been shown.
    pub welcomed: bool,
}

pub const DEFAULTS: Settings = Settings { enabled: true, step: 5, welcomed: false };

impl Default for Settings {
    fn default() -> Self {
        DEFAULTS
    }
}

static CURRENT: RwLock<Settings> = RwLock::new(DEFAULTS);

pub fn current() -> Settings {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// Loads the saved settings (defaults if there are none).
pub fn load() {
    let loaded = std::fs::read_to_string(config_dir().join("settings.ini")).map(|t| from_text(&t)).unwrap_or_default();
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = loaded;
}

/// Changes the settings and saves them.
pub fn update(change: impl FnOnce(&mut Settings)) {
    let updated = {
        let mut s = CURRENT.write().unwrap_or_else(|e| e.into_inner());
        change(&mut s);
        *s
    };
    let dir = config_dir();
    let (path, tmp) = (dir.join("settings.ini"), dir.join("settings.ini.tmp"));
    let saved = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&tmp, to_text(&updated)))
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = saved {
        super::log::write(&format!("couldn't save settings: {e}"));
    }
}

/// Per-user folder for the settings and the log.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));

    base.unwrap_or_else(std::env::temp_dir).join(APP_NAME)
}

fn to_text(s: &Settings) -> String {
    format!("enabled = {}\nstep = {}\nwelcomed = {}\n", s.enabled, s.step, s.welcomed)
}

fn from_text(text: &str) -> Settings {
    let mut s = DEFAULTS;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "enabled" => s.enabled = value.parse().unwrap_or(s.enabled),
            "step" => s.step = value.parse::<u32>().map(|v| v.clamp(1, 25)).unwrap_or(s.step),
            "welcomed" => s.welcomed = value.parse().unwrap_or(s.welcomed),
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings { enabled: false, step: 2, welcomed: true };
        assert_eq!(from_text(&to_text(&s)), s);
    }

    #[test]
    fn ignores_junk_and_clamps_the_step() {
        let s = from_text("# comment\nenabled = nope\nstep = 400\nunknown = 1\n");
        assert_eq!(s, Settings { step: 25, ..DEFAULTS });
    }
}
