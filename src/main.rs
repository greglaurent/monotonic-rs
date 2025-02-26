#![allow(dead_code, unused)]
// TODO  Write tests
// TODO struct Realtime -- let time = match nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALMONO_TIME) {
// TODO:const WINDOWS: &'static str = "windows"
// TODO: Crash safely, or recover

use std::time::Duration;

use time::{Clock, Fidelity};

fn main() {
    let clock = Clock::default();

    loop {
        let x = clock.tick();
        println!("{x}");

        std::thread::sleep(Duration::new(2, 0));
    }
}

pub mod time {
    use nix::{libc::adjtime, sys::time::TimeSpec};
    use std::{
        env::consts::OS,
        sync::{
            atomic::{AtomicU64, Ordering},
            LazyLock, OnceLock,
        },
        u64,
    };

    const LINUX: &str = "linux";
    const WINDOWS: &str = "linux"; //FIXME, just for testing
    const RUN_OS: &str = std::env::consts::OS;

    const NANOS_PER_SEC: u64 = 1_000_000_000;
    const NANOS_PER_MILLI: u64 = 1_000_000;

    static RUN_ON: OnceLock<u64> = OnceLock::<u64>::new();

    static MONO_TIME: AtomicU64 = AtomicU64::new(0);
    static REAL_TIME: AtomicU64 = AtomicU64::new(0);

    static FIDELITY: AtomicU64 = AtomicU64::new(0);

    /// Controls the cadence of the clock sweep.
    /// ```
    /// // scales each tick to 16 milliseconds.
    /// Fidelity::Millis(16);
    /// ```
    #[derive(Debug, Clone)]
    pub enum Fidelity {
        Nanos(usize),
        Millis(usize),
        Seconds(usize),
    }

    impl Fidelity {
        /// Calculates the divisor to scale Clock ticks.
        fn divisor(&self) -> u64 {
            match self {
                Fidelity::Nanos(n) => *n as u64,
                Fidelity::Millis(m) => *m as u64 * NANOS_PER_MILLI,
                Fidelity::Seconds(s) => *s as u64 * NANOS_PER_SEC,
            }
        }
    }

    /// Clock to read time based on an OS-based value
    /// when the application starts.
    ///
    /// ```
    /// // OS value set by rust available at runtime.
    /// const RUN_OS: &str = std::env::consts::OS;
    /// ```
    ///
    /// Darwin => TODO
    /// Windows => TODO
    /// Linux => CLOCK_BOOTTIME -- see note on the impl
    /// why CLOCK_BOOTTIME instead of CLOCK_MONOTONIC
    ///
    /// ```
    /// // These are the same for rnitialization.
    /// // Clock::new() calls default() and updates
    /// // fields set by parameters.
    /// let default = Clock::default();
    /// let new = Clock::new(Fidelity::Millis(16));
    ///
    /// assert_eq!(new.div, default.div, "Fidelity between default() and new() are not the same.");
    /// ```
    pub struct Clock {
        div: AtomicU64,
        mono: OnceLock<Monotonic>,
    }

    impl Default for Clock {
        /// Initializes the default Clock and setups up time
        /// based on the detected OS.
        fn default() -> Self {
            let mono: Monotonic = match RUN_OS {
                LINUX => init::<Linux>(),
                _ => panic!("blah"),
            };

            let mono_lock = OnceLock::new();
            let m = mono_lock.get_or_init(|| mono);

            let m_time = m.hw_time();
            RUN_ON.get_or_init(|| m_time);
            MONO_TIME.swap(m_time, Ordering::AcqRel);

            Clock {
                div: AtomicU64::new(Fidelity::Millis(16).divisor()),
                mono: mono_lock,
            }
        }
    }

    impl Clock {
        /// Initializes an new Clock and sets up OS-based time.
        ///
        /// ```
        /// // Clock::new() calls default() and updates
        /// // fields set from parameters.
        /// let new = Clock::new(Fidelity::Millis(16));
        ///
        /// assert_eq!(new.div, default.div, "Fidelity between default() and new() are not the same.");
        /// ```
        pub fn new(f: Fidelity) -> Self {
            let mut clock = Clock::default();
            clock.sweep(f);

            clock
        }

        /// Update the fidelity (scale) of the tick.
        pub fn sweep(&mut self, f: Fidelity) {
            self.div = AtomicU64::new(f.divisor());
        }

        /// Total number of scaled ticks from application start.
        ///
        /// Tick the application clock, then
        /// OS-based time at application start divided by self.div
        pub fn real_tick(&self) -> u64 {
            self.tick();
            REAL_TIME.load(Ordering::Acquire) / self.div.load(Ordering::Acquire)
        }

        /// Ticks the clock based on elapsed time from applicaiton start.
        pub fn tick(&self) -> u64 {
            let m = match self.mono.get() {
                Some(t) => t.elapsed(),
                None => panic!("fix this"),
            };

            m / self.div.load(Ordering::Acquire)
        }
    }

    fn init<O: OsType>() -> Monotonic {
        // other time type initialization
        Monotonic::new()
    }

    trait OsType {}

    struct Linux;
    impl OsType for Linux {}

    trait OsTime<O: OsType> {
        fn new() -> Self;

        fn hw_time(&self) -> u64;

        fn adjust_time(time: TimeSpec) -> u64 {
            let secs = time.tv_sec();
            let nsec = time.tv_nsec();

            // Account for fractional time with nanoseconds;
            // Avoids trimming scaled time.
            let t = secs as u128 * NANOS_PER_SEC as u128 + nsec as u128;

            t as u64
        }

        fn elapsed(&self) -> u64 {
            let t = self.hw_time() - RUN_ON.get().unwrap();
            self.set_global(t);

            t
        }

        fn set_global(&self, t: u64);
    }

    struct Monotonic;
    impl OsTime<Linux> for Monotonic {
        fn new() -> Self {
            assert!(RUN_OS == LINUX, "blah");

            Self
        }

        /// Use CLOCK_BOOTTIME instead of CLOCK_MONOTONIC.
        /// CLOCK_MONOTONIC excludes elapsed time while the system is suspended.
        ///
        /// CLOCK_BOOTTIME includes elapsed time while suspended;
        /// It is the true monotonic clock in Linux.
        fn hw_time(&self) -> u64 {
            assert!(LINUX == RUN_OS, "Failed OS check. Invalid OS path."); // should not happen

            let time = match nix::time::clock_gettime(nix::time::ClockId::CLOCK_BOOTTIME) {
                Ok(t) => t,
                Err(_) => panic!("System CLOCK_BOOTMONO_TIME is required."),
            };

            let time = Self::adjust_time(time);
            let old = REAL_TIME.swap(time, Ordering::AcqRel);

            // Hardware and kernel bugs may regress the monotonic clock.
            // Crash safely rather than possibly creating infinite loops.
            assert!(
                old <= time,
                "Hardware/kernel bug regressed the monotonic clock. Crashing is better than infinity."
            );

            time
        }

        fn set_global(&self, t: u64) {
            let _ = MONO_TIME.swap(t, Ordering::AcqRel);
        }
    }
}
