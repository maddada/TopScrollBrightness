//! Monitor brightness over DDC/CI (VCP code 0x10, the setting in the monitor's
//! own menu and in tools like ASUS DisplayWidget Center).
//!
//! Talking to a monitor is slow (a read takes ~70 ms, a write ~55 ms), so it
//! happens on a worker thread. The hook only updates the wanted level, and the
//! indicator shows it right away; the worker writes the latest wanted level
//! whenever the previous write has finished, skipping the ones in between.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, PHYSICAL_MONITOR, SetVCPFeature,
};
use windows_sys::Win32::Graphics::Gdi::HMONITOR;

use super::osd;
use crate::shared::level::Level;
use crate::shared::log;

const VCP_BRIGHTNESS: u8 = 0x10;

/// After this long without scrolling, the monitor's level is read again on the
/// next scroll, in case it was changed elsewhere (its buttons, DisplayWidget Center…).
const FORGET_AFTER: Duration = Duration::from_secs(3);

/// How long a monitor that didn't answer is left alone before trying again.
const RETRY_UNSUPPORTED_AFTER: Duration = Duration::from_secs(60);

/// DDC/CI replies are occasionally garbled; reads are retried this many times.
const READ_ATTEMPTS: usize = 3;

#[derive(Clone)]
pub struct Brightness(Arc<Shared>);

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    /// The monitor being adjusted (an `HMONITOR`), 0 for none.
    monitor: isize,
    /// The wanted level, once the monitor's current level is known.
    level: Option<Level>,
    /// Scrolling (in percent) that arrived before the current level was known.
    pending: f64,
    last_input: Option<Instant>,
    /// Monitors that didn't answer a brightness request, and when; scrolling
    /// there passes through for a while.
    unsupported: Vec<(isize, Instant)>,
}

pub fn spawn() -> Brightness {
    let shared = Arc::new(Shared { state: Mutex::new(State::default()), wake: Condvar::new() });
    let worker = shared.clone();
    std::thread::Builder::new()
        .name("brightness".into())
        .spawn(move || run(&worker))
        .expect("failed to spawn brightness thread");
    Brightness(shared)
}

impl Brightness {
    /// Whether scrolling on this monitor should change its brightness.
    pub fn handles(&self, monitor: isize) -> bool {
        let mut st = self.0.lock();
        st.unsupported.retain(|(_, since)| since.elapsed() < RETRY_UNSUPPORTED_AFTER);
        !st.unsupported.iter().any(|&(m, _)| m == monitor)
    }

    /// Changes the monitor's brightness by `percent` of its range.
    pub fn nudge(&self, monitor: isize, percent: f64) {
        let mut st = self.0.lock();
        if st.monitor != monitor {
            st.monitor = monitor;
            st.level = None;
            st.pending = 0.0;
        }
        st.last_input = Some(Instant::now());
        match st.level.as_mut() {
            Some(level) => {
                level.nudge(percent);
                osd::show(monitor, level.fraction(), level.device_value());
            }
            None => st.pending += percent,
        }
        drop(st);
        self.0.wake.notify_one();
    }

    /// Displays were added, removed or rearranged: monitor handles have changed.
    pub fn displays_changed(&self) {
        *self.0.lock() = State::default();
        self.0.wake.notify_one();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

enum Job {
    Read(isize),
    Write(u32),
}

fn run(shared: &Shared) {
    let mut open: Option<PhysicalMonitors> = None;
    let mut written: Option<u32> = None;
    loop {
        let job = {
            let mut st = shared.lock();
            loop {
                if st.monitor != 0 && st.level.is_none() && st.pending != 0.0 {
                    break Job::Read(st.monitor);
                }
                if let Some(level) = st.level
                    && written != Some(level.device_value())
                    && open.as_ref().is_some_and(|o| o.monitor == st.monitor)
                {
                    break Job::Write(level.device_value());
                }
                if st.last_input.is_some_and(|t| t.elapsed() >= FORGET_AFTER) {
                    let unsupported = std::mem::take(&mut st.unsupported);
                    *st = State { unsupported, ..State::default() };
                    open = None;
                    written = None;
                }
                st = shared.wake.wait_timeout(st, Duration::from_secs(1)).unwrap_or_else(|e| e.into_inner()).0;
            }
        };

        match job {
            Job::Read(monitor) => {
                open = PhysicalMonitors::open(monitor);
                let current = open.as_ref().and_then(PhysicalMonitors::read);
                let mut st = shared.lock();
                if st.monitor != monitor {
                    continue; // the pointer moved to another monitor meanwhile
                }
                match current {
                    Some((value, max)) => {
                        let mut level = Level::new(value, max);
                        level.nudge(std::mem::take(&mut st.pending));
                        st.level = Some(level);
                        written = Some(value);
                        osd::show(monitor, level.fraction(), level.device_value());
                    }
                    None => {
                        log::write("a monitor didn't answer a DDC/CI brightness request; leaving it alone");
                        st.unsupported.push((monitor, Instant::now()));
                        st.monitor = 0;
                        st.pending = 0.0;
                        open = None;
                    }
                }
            }
            Job::Write(value) => {
                if let Some(o) = &open {
                    o.write(value);
                }
                written = Some(value);
            }
        }
    }
}

/// The physical monitors behind one display (more than one when mirrored).
struct PhysicalMonitors {
    monitor: isize,
    list: Vec<PHYSICAL_MONITOR>,
}

impl PhysicalMonitors {
    fn open(monitor: isize) -> Option<Self> {
        let hmonitor = monitor as HMONITOR;
        let mut count = 0;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmonitor, &mut count) } == 0 || count == 0 {
            return None;
        }
        let mut list = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hmonitor, count, list.as_mut_ptr()) } == 0 {
            return None;
        }
        Some(Self { monitor, list })
    }

    /// The current brightness and its maximum, from the first monitor that answers.
    fn read(&self) -> Option<(u32, u32)> {
        (0..READ_ATTEMPTS).find_map(|attempt| {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(40));
            }
            self.list.iter().find_map(|m| {
                let handle = m.hPhysicalMonitor; // copied out: the struct is packed
                let (mut current, mut max, mut kind) = (0, 0, 0);
                let ok =
                    unsafe { GetVCPFeatureAndVCPFeatureReply(handle, VCP_BRIGHTNESS, &mut kind, &mut current, &mut max) };
                (ok != 0 && max > 0).then_some((current, max))
            })
        })
    }

    fn write(&self, value: u32) {
        for m in &self.list {
            let handle = m.hPhysicalMonitor;
            unsafe { SetVCPFeature(handle, VCP_BRIGHTNESS, value) };
        }
    }
}

impl Drop for PhysicalMonitors {
    fn drop(&mut self) {
        unsafe { DestroyPhysicalMonitors(self.list.len() as u32, self.list.as_ptr()) };
    }
}
