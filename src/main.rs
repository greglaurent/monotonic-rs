use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use nix::sys::time::TimeSpec;

const LINUX: &str = "linux";
//TODO: const WINDOWS: &'static str = "windows";

const NANOS_PER_SEC: u64 = 1_000_000_000;
const NANOS_PER_MILLI: u64 = 1_000_000;
const RUN_OS: &str = std::env::consts::OS;

static REALTIME: AtomicU64 = AtomicU64::new(0);
static REALTIME_GUARD: AtomicU64 = AtomicU64::new(0);

static MONOTONIC: AtomicU64 = AtomicU64::new(0);
static MONOTONIC_GUARD: AtomicU64 = AtomicU64::new(0);

pub struct Time;
impl Time {
    pub fn monotonic() -> u64 {
        let new = match RUN_OS {
            LINUX => Self::monotonic_linux(),
            _ => panic!("Unsupported OS -- Please use Linux"),
        };

        // TODO: Crash safely, or recover
        // Hardware and kernel bugs may regress the monotonic clock.
        // Crash safely rather than possibly creating infinite loops.
        assert!(
            MONOTONIC_GUARD.load(Ordering::SeqCst) <= MONOTONIC.load(Ordering::SeqCst)
                && MONOTONIC.load(Ordering::SeqCst) <= new,
            "Hardware/kernel bug regressed the monotonic clock. Crashing is better than infinity."
        );

        let _ = MONOTONIC.swap(new, Ordering::SeqCst);
        if MONOTONIC_GUARD.load(Ordering::SeqCst) == 0 {
            let _ = MONOTONIC_GUARD.swap(new, Ordering::SeqCst);
        }

        new
    }

    pub fn realtime() -> u64 {
        let new = match RUN_OS {
            LINUX => Self::realtime_unix(),
            _ => panic!("Unsupported OS -- Please use Linux"),
        };

        // TODO: Crash safely, or recover
        // Hardware and kernel bugs may prevent checking CLOCK_REALTIME.
        // Crash safely rather than being able to read time.
        assert!(
            REALTIME_GUARD.load(Ordering::SeqCst) <= REALTIME.load(Ordering::SeqCst)
                && REALTIME.load(Ordering::SeqCst) <= new,
            "Hardware/kernel bug regressed the monotonic clock. Crashing is better than infinity."
        );

        let _ = REALTIME.swap(new, Ordering::SeqCst);
        if REALTIME_GUARD.load(Ordering::SeqCst) == 0 {
            let _ = REALTIME_GUARD.swap(new, Ordering::SeqCst);
        }

        new
    }

    pub fn elapsed() -> u64 {
        let now = Self::monotonic();
        now - MONOTONIC_GUARD.load(Ordering::SeqCst)
    }

    pub fn elapsed_rt() -> u64 {
        let now = Self::realtime();
        now - REALTIME_GUARD.load(Ordering::SeqCst)
    }

    pub fn ticks(fidelity: Fidelity) -> u64 {
        let div = match fidelity {
            Fidelity::Nanos(n) => n as u64,
            Fidelity::Millis(m) => m as u64 * NANOS_PER_MILLI,
            Fidelity::Seconds(s) => s as u64 * NANOS_PER_SEC,
        };

        let now = Self::elapsed();

        now / div
    }

    fn monotonic_linux() -> u64 {
        assert!(LINUX == RUN_OS, "Failed OS check. Invalid OS path."); // should not happen

        // Use CLOCK_BOOTTIME instead of CLOCK_MONOTONIC
        // CLOCK_MONOTONIC excludes elapsed time while the system is suspended.
        // CLOCK_BOOTTIME includes elapsed time while suspended;
        // It is the true monotonic clock in Linux.
        let time = match nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME) {
            Ok(t) => t,
            Err(_) => panic!("System CLOCK_BOOTTIME is required."),
        };

        Self::adjust_time(time)
    }

    fn realtime_unix() -> u64 {
        assert!(LINUX == RUN_OS, "Failed OS check. Invalid OS path."); // should not happen
        let time = match nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME) {
            Ok(t) => t,
            Err(_) => panic!("System CLOCK_REALTIME is required."),
        };

        let rt = Self::adjust_time(time);
        REALTIME.store(rt, Ordering::SeqCst);

        rt
    }

    const fn adjust_time(time: TimeSpec) -> u64 {
        let secs = time.tv_sec();
        let nsec = time.tv_nsec();

        // Account for fractional time with nanoseconds; avoids trimming scaled time;
        let t = secs as u128 * NANOS_PER_SEC as u128 + nsec as u128;

        t as u64
    }
}

pub enum Fidelity {
    Nanos(usize),
    Millis(usize),
    Seconds(usize),
}

fn main() {
    let dur = Duration::from_secs(1);

    let max = 100;
    let mut count = 0;
    loop {
        let m = Time::ticks(Fidelity::Millis(16));
        println!("GUARD: {MONOTONIC_GUARD:?} TICKS {:?}", m);

        count += 1;
        if count > max {
            break;
        }

        std::thread::sleep(dur);
    }
}
