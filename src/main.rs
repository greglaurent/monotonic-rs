use std::time::Duration;

use time::{Fidelity, MonoClock};

pub mod time {
    use nix::sys::time::TimeSpec;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const LINUX: &str = "linux";
    //TODO: const WINDOWS: &'static str = "windows";

    const NANOS_PER_SEC: u64 = 1_000_000_000;
    const NANOS_PER_MILLI: u64 = 1_000_000;
    const RUN_OS: &str = std::env::consts::OS;

    static RUNNING: AtomicBool = AtomicBool::new(false);

    static REALTIME: AtomicU64 = AtomicU64::new(0);
    static REALTIME_GUARD: AtomicU64 = AtomicU64::new(0);

    static MONOTONIC: AtomicU64 = AtomicU64::new(0);
    static MONOTONIC_GUARD: AtomicU64 = AtomicU64::new(0);

    struct Time;
    impl Time {
        fn up() {
            assert!(
                MONOTONIC_GUARD.load(Ordering::SeqCst) == 0,
                "Time only needs to be initialized once."
            );

            assert!(
                REALTIME_GUARD.load(Ordering::SeqCst) == 0,
                "Time only needs to be initialized once."
            );

            RUNNING.store(true, Ordering::SeqCst);

            Self::monotonic();

            Self::realtime();
        }

        fn monotonic() -> u64 {
            println!("{:?}", RUNNING.load(Ordering::SeqCst));
            assert!(
                RUNNING.load(Ordering::SeqCst),
                "Time must be initialized with Time::new()."
            );

            let new = match RUN_OS {
                LINUX => Self::monotonic_linux(),
                _ => panic!("Unsupported OS -- Please use Linux"),
            };

            let _ = MONOTONIC.swap(new, Ordering::SeqCst);

            // TODO: Crash safely, or recover
            // Hardware and kernel bugs may regress the monotonic clock.
            // Crash safely rather than possibly creating infinite loops.
            assert!(
            MONOTONIC_GUARD.load(Ordering::SeqCst) <= MONOTONIC.load(Ordering::SeqCst)
                && MONOTONIC.load(Ordering::SeqCst) <= new,
                "Hardware/kernel bug regressed the monotonic clock. Crashing is better than infinity."
            );

            new
        }

        fn realtime() -> u64 {
            assert!(
                RUNNING.load(Ordering::SeqCst),
                "Time must be initialized with Time::new()."
            );

            let new = match RUN_OS {
                LINUX => Self::realtime_unix(),
                _ => panic!("Unsupported OS -- Please use Linux"),
            };

            let _ = REALTIME.swap(new, Ordering::SeqCst);

            // TODO: Crash safely, or recover
            // Hardware and kernel bugs may prevent checking CLOCK_REALTIME.
            // Crash safely rather than being able to read time.
            assert!(
            REALTIME_GUARD.load(Ordering::SeqCst) <= REALTIME.load(Ordering::SeqCst)
                && REALTIME.load(Ordering::SeqCst) <= new,
                "Hardware/kernel bug regressed the monotonic clock. Crashing is better than infinity."
            );

            new
        }

        pub fn elapsed() -> u64 {
            let now = Self::monotonic();
            now - MONOTONIC_GUARD.load(Ordering::SeqCst)
        }

        pub fn elapsed_from(from: u64) -> u64 {
            let now = Self::elapsed();
            now - from - MONOTONIC_GUARD.load(Ordering::SeqCst)
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

    #[derive(Debug, Clone, Copy)]
    pub enum Fidelity {
        Nanos(usize),
        Millis(usize),
        Seconds(usize),
    }

    #[derive(Debug, Clone, Copy)]
    pub struct MonoClock {
        started_at: u64,
        stopped_at: Option<u64>,
        fidelity: Fidelity,
    }

    impl MonoClock {
        pub fn new(fidelity: Fidelity) -> Self {
            if !RUNNING.load(Ordering::SeqCst) {
                Time::up();
            }

            Self {
                fidelity,
                started_at: Time::monotonic(),
                stopped_at: None,
            }
        }

        pub fn started_at(&self) -> u64 {
            self.started_at
        }

        pub fn stop(&mut self) {
            self.stopped_at = Some(Time::monotonic());
        }

        pub fn stopped_at(&self) -> Option<u64> {
            self.stopped_at
        }

        pub fn ticks(&self) -> u64 {
            Self::ticks_with(self.started_at, &self.fidelity)
        }

        pub fn ticks_epoch(&self) -> u64 {
            Self::ticks_with(0, &self.fidelity)
        }

        fn ticks_with(from: u64, fidelity: &Fidelity) -> u64 {
            let div = match fidelity {
                Fidelity::Nanos(n) => *n as u64,
                Fidelity::Millis(m) => *m as u64 * NANOS_PER_MILLI,
                Fidelity::Seconds(s) => *s as u64 * NANOS_PER_SEC,
            };

            let now = Time::elapsed_from(from);

            now / div
        }
    }
}

fn main() {
    let dur = Duration::from_secs(1);

    let max = 100;
    let mut count = 0;
    let m = MonoClock::new(Fidelity::Millis(16));

    loop {
        println!("Clock: {m:?} TICKS {:?}", m.ticks());

        count += 1;
        if count > max {
            break;
        }

        std::thread::sleep(dur);
    }
}
