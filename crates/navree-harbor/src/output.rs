//! Progress lines and time: what Harbor prints while it works, and the
//! clock it reads (fixed in tests, so output is reproducible).

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

/// Where progress lines go.
pub trait Progress: Send + Sync {
    /// Prints one line.
    fn line(&self, text: &str);

    /// `  <step>  <detail>`, the step padded to 9 columns.
    fn step(&self, step: &str, detail: &str) {
        self.line(&format!("  {step:<9} {detail}"));
    }

    /// `  <step>  <server>  <detail>`.
    fn server_step(&self, step: &str, server: &str, detail: &str) {
        self.line(&format!("  {step:<9} {server}  {detail}"));
    }
}

/// Prints to standard output.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stdout;

impl Progress for Stdout {
    fn line(&self, text: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    }
}

/// Collects lines (tests).
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    lines: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every line so far.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lines lock").clone()
    }

    /// Every line so far, joined with newlines.
    pub fn text(&self) -> String {
        self.lines().join("\n")
    }
}

impl Progress for Recorder {
    fn line(&self, text: &str) {
        self.lines.lock().expect("lines lock").push(text.to_owned());
    }
}

/// The current time.
pub trait Clock: Send + Sync {
    /// Now, in UTC.
    fn now(&self) -> DateTime<Utc>;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock stopped at one instant (tests).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub DateTime<Utc>);

impl FixedClock {
    /// 2026-10-26 09:15:12 UTC.
    pub fn example() -> Self {
        Self(
            DateTime::parse_from_rfc3339("2026-10-26T09:15:12Z")
                .expect("a valid instant")
                .with_timezone(&Utc),
        )
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

/// `1m 53s`, `6s`, `2h 4m`.
pub fn duration(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, s % 3600 / 60),
    }
}

/// Seconds between two instants.
pub fn elapsed(clock: &dyn Clock, since: DateTime<Utc>) -> String {
    duration((clock.now() - since).num_seconds())
}

/// `14.2 MB`, `830 KB`, `512 B`.
pub fn size(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    match bytes {
        0..KB => format!("{bytes} B"),
        KB..MB => format!("{} KB", bytes / KB),
        MB..GB => format!("{:.1} MB", bytes as f64 / MB as f64),
        _ => format!("{:.1} GB", bytes as f64 / GB as f64),
    }
}

/// `0.4 s` from milliseconds.
pub fn millis(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_sizes() {
        assert_eq!(duration(6), "6s");
        assert_eq!(duration(113), "1m 53s");
        assert_eq!(duration(192), "3m 12s");
        assert_eq!(duration(7440), "2h 4m");
        assert_eq!(duration(-3), "0s");
        assert_eq!(size(512), "512 B");
        assert_eq!(size(830_000), "830 KB");
        assert_eq!(size(14_200_000), "14.2 MB");
        assert_eq!(size(2_500_000_000), "2.5 GB");
        assert_eq!(millis(400), "0.4 s");
        assert_eq!(millis(1250), "1.2 s");
    }

    #[test]
    fn steps_line_up() {
        let recorder = Recorder::new();
        recorder.step("base", "updates");
        recorder.server_step("upload", "app-1", "releases/r1");
        assert_eq!(
            recorder.lines(),
            ["  base      updates", "  upload    app-1  releases/r1"]
        );
    }

    #[test]
    fn the_fixed_clock_stands_still() {
        let clock = FixedClock::example();
        assert_eq!(elapsed(&clock, clock.now()), "0s");
        assert_eq!(
            clock.now().format("%Y%m%d-%H%M%S").to_string(),
            "20261026-091512"
        );
    }
}
