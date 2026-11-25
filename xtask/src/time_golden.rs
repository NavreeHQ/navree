//! `cargo xtask time-golden [--check]`: records how Navree writes and reads
//! dates and times today, as the golden files in
//! `crates/navree-datetime/tests/golden/`.
//!
//! Every date and time in Navree is a chrono value today, and SQLx's chrono
//! support writes it to a database. This program takes a fixed sweep of
//! values through every way one is written (`Display`, `Debug`, RFC 3339,
//! RFC 2822, JSON, each `strftime` specifier, and what goes to PostgreSQL,
//! MySQL and SQLite) and a fixed set of texts through every way one is read,
//! and writes one JSON object per line. Navree's own date and time library
//! is tested against these files, so that nothing stored, sent or shown
//! changes when it takes chrono's place.
//!
//! `README.md` in that folder, which this program also writes, says what
//! each file and each field holds. Nothing here reads a clock, a locale or
//! the environment, so every run writes the same bytes.
//!
//! This is the `time-golden` program of the `xtask` package, built only with
//! the `time-golden` feature; `cargo xtask time-golden` builds and runs it.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use chrono::format::StrftimeItems;
use chrono::{
    DateTime, Datelike, FixedOffset, LocalResult, Month, NaiveDate, NaiveDateTime, NaiveTime,
    Offset, SecondsFormat, TimeDelta, TimeZone, Timelike, Utc, Weekday,
};
use navree_time::{LocalDay, Tz, Zone};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::mysql::MySql;
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, Postgres};
use sqlx::sqlite::{Sqlite, SqliteArgumentsBuffer};
use sqlx::{Encode, Type, TypeInfo};
use xtask::time::{civil_from_days, days_from_civil};

/// The golden files' folder, from the workspace root.
const DIR: &str = "crates/navree-datetime/tests/golden";

const DISPLAY: &str = "display.jsonl";
const RFC3339: &str = "rfc3339.jsonl";
const RFC2822: &str = "rfc2822.jsonl";
const SERDE: &str = "serde.jsonl";
const STRFTIME: &str = "strftime.jsonl";
const PATTERNS: &str = "patterns.jsonl";
const PARSE_ACCEPT: &str = "parse_accept.jsonl";
const PARSE_REFUSE: &str = "parse_refuse.jsonl";
const LOCAL: &str = "local.jsonl";
const SQLITE: &str = "sqlite.jsonl";
const POSTGRES: &str = "postgres.jsonl";
const MYSQL: &str = "mysql.jsonl";

/// Every file written, in the order the README lists them.
const FILES: [&str; 12] = [
    DISPLAY,
    RFC3339,
    RFC2822,
    SERDE,
    STRFTIME,
    PATTERNS,
    PARSE_ACCEPT,
    PARSE_REFUSE,
    LOCAL,
    SQLITE,
    POSTGRES,
    MYSQL,
];

const NANOS: u32 = 1_000_000_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let check = match args.as_slice() {
        [] => false,
        ["--check"] => true,
        other => {
            eprintln!(
                "error: unknown arguments: {}\n\ncargo xtask time-golden [--check]",
                other.join(" ")
            );
            return ExitCode::FAILURE;
        }
    };
    match run(&root(), check) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root: this package's parent folder.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is in the workspace root")
        .to_owned()
}

/// Writes the golden files and their README, or with `check` writes nothing
/// and says which differ.
fn run(root: &Path, check: bool) -> Result<ExitCode, String> {
    quiet_panics();
    let golden = generate()?;
    let dir = root.join(DIR);
    let mut outputs: Vec<(String, String)> = Vec::new();
    for name in FILES {
        let lines = golden.files.get(name).map(Vec::as_slice).unwrap_or(&[]);
        if lines.is_empty() {
            return Err(format!("{name} came out empty"));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        outputs.push((name.to_owned(), text));
    }
    outputs.push(("README.md".to_owned(), readme(root, &golden)?));

    let mut stale = Vec::new();
    for (name, text) in &outputs {
        let path = dir.join(name);
        let on_disk = std::fs::read_to_string(&path)
            .map(|s| s.replace("\r\n", "\n"))
            .ok();
        if on_disk.as_deref() == Some(text.as_str()) {
            continue;
        }
        if check {
            stale.push(name.as_str());
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    if check && !stale.is_empty() {
        for name in stale {
            eprintln!("- {DIR}/{name} is stale");
        }
        eprintln!("write them again with `cargo xtask time-golden`");
        return Ok(ExitCode::FAILURE);
    }
    for name in FILES {
        let lines = golden.files.get(name).map_or(0, Vec::len);
        println!("{lines:>7} lines  {DIR}/{name}");
    }
    println!(
        "{}",
        if check {
            "the golden files are what this program writes"
        } else {
            "wrote the golden files and their README.md"
        }
    );
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

/// Everything to write: each file's lines.
#[derive(Default)]
struct Golden {
    files: BTreeMap<&'static str, Vec<String>>,
    /// Every line of every file, so a value that is in two sets of the sweep
    /// is written once.
    lines: BTreeSet<(&'static str, String)>,
    /// The texts already given to a way of reading, so one that comes up
    /// twice is recorded once.
    read: BTreeSet<String>,
    /// Every pattern a value was written or a text was read with, in the
    /// order they first came up: all of them go in `patterns.jsonl`.
    patterns: Vec<String>,
}

impl Golden {
    fn push(&mut self, file: &'static str, row: Row) {
        let line = row.end();
        if self.lines.insert((file, line.clone())) {
            self.files.entry(file).or_default().push(line);
        }
    }

    fn used(&mut self, pattern: &str) {
        if !self.patterns.iter().any(|known| known == pattern) {
            self.patterns.push(pattern.to_owned());
        }
    }
}

/// One line: a JSON object whose keys stay in the order they were added, in
/// ASCII only (anything else is written as a `\u` escape, so a no-break
/// space or a minus sign shows in a diff).
struct Row(String);

impl Row {
    fn of(ty: &str) -> Row {
        Row(String::from("{")).text("type", ty)
    }

    fn key(&mut self, key: &str) {
        if self.0.len() > 1 {
            self.0.push(',');
        }
        json_text(&mut self.0, key);
        self.0.push(':');
    }

    fn text(mut self, key: &str, value: &str) -> Row {
        self.key(key);
        json_text(&mut self.0, value);
        self
    }

    fn int(mut self, key: &str, value: i64) -> Row {
        self.key(key);
        let _ = write!(self.0, "{value}");
        self
    }

    fn flag(mut self, key: &str, value: bool) -> Row {
        self.key(key);
        self.0.push_str(if value { "true" } else { "false" });
        self
    }

    /// A list of texts.
    fn list(mut self, key: &str, values: &[&str]) -> Row {
        self.key(key);
        self.0.push('[');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.0.push(',');
            }
            json_text(&mut self.0, value);
        }
        self.0.push(']');
        self
    }

    /// `out` when the call gave a text, `err` when it failed.
    fn outcome(self, outcome: &Outcome) -> Row {
        match outcome {
            Ok(text) => self.text("out", text),
            Err(why) => self.text("err", why),
        }
    }

    fn end(mut self) -> String {
        self.0.push('}');
        self.0
    }
}

fn json_text(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// Calls that may fail
// ---------------------------------------------------------------------------

/// What a call wrote, or why it wrote nothing: `format` when the formatter
/// returned an error (where `to_string()` panics), `panic` when the call
/// itself panicked.
type Outcome = Result<String, &'static str>;

thread_local! {
    /// Set while a call that may panic runs, so its panic prints nothing.
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// Keeps the panics that [`guard`] catches off the terminal; any other panic
/// prints as usual.
fn quiet_panics() {
    let usual = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !QUIET.with(Cell::get) {
            usual(info);
        }
    }));
}

/// Runs `call`; `Err` when it panicked.
fn guard<T>(call: impl FnOnce() -> T) -> Result<T, ()> {
    QUIET.with(|quiet| quiet.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(call));
    QUIET.with(|quiet| quiet.set(false));
    result.map_err(|_| ())
}

/// The text `write` writes.
fn written(write: impl FnOnce(&mut String) -> fmt::Result) -> Outcome {
    let mut text = String::new();
    match guard(|| write(&mut text)) {
        Ok(Ok(())) => Ok(text),
        Ok(Err(_)) => Err("format"),
        Err(()) => Err("panic"),
    }
}

/// The text `call` returns.
fn called(call: impl FnOnce() -> String) -> Outcome {
    guard(call).map_err(|()| "panic")
}

// ---------------------------------------------------------------------------
// Values and how a line names them
// ---------------------------------------------------------------------------

/// A value of one of the recorded types: its name in the files, its
/// canonical text, and the same value as numbers.
trait Value: Copy {
    const TYPE: &'static str;

    /// The canonical text (the `value` of a line that writes this value, the
    /// `out` of a line that read it).
    fn canon(&self) -> String;

    /// The value as numbers, so a test needs no parser to build it.
    fn parts(&self, row: Row) -> Row;

    /// A line that starts with this value.
    fn row(&self) -> Row {
        self.parts(Row::of(Self::TYPE).text("value", &self.canon()))
    }
}

/// `2026-01-15`; a year outside 0 to 9999 has a sign and at least four
/// digits (`+262142-12-31`, `-0001-01-01`).
fn date_text(year: i64, month: u32, day: u32) -> String {
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+05}")
    };
    format!("{year}-{month:02}-{day:02}")
}

/// `09:30:00.000000000`, always nine digits; a leap second (nanoseconds of
/// one second or more) shows as the next second, so 23:59:59 with
/// 1,500,000,000 ns is `23:59:60.500000000`.
fn time_text(hour: i64, minute: i64, second: i64, nanos: u32) -> String {
    let (second, nanos) = if nanos >= NANOS {
        (second + 1, nanos - NANOS)
    } else {
        (second, nanos)
    };
    format!("{hour:02}:{minute:02}:{second:02}.{nanos:09}")
}

/// `+05:30`, with the seconds when there are any (`+05:53:28`).
fn offset_text(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let abs = seconds.unsigned_abs();
    let (hours, minutes, rest) = (abs / 3600, abs / 60 % 60, abs % 60);
    if rest == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{rest:02}")
    }
}

/// An instant's canonical text: its local date and time, `Z` for UTC or the
/// offset, and the zone's name in brackets when it has one. Worked out from
/// the seconds alone, so it is right at the ends of the range too.
fn instant_text(secs: i64, nanos: u32, offset: Option<i32>, zone: Option<&str>) -> String {
    let local = secs + i64::from(offset.unwrap_or(0));
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let of_day = local.rem_euclid(86_400);
    let mut text = format!(
        "{}T{}",
        date_text(year, month, day),
        time_text(of_day / 3600, of_day / 60 % 60, of_day % 60, nanos)
    );
    match offset {
        None => text.push('Z'),
        Some(offset) => text.push_str(&offset_text(offset)),
    }
    if let Some(zone) = zone {
        let _ = write!(text, "[{zone}]");
    }
    text
}

fn instant_parts(row: Row, secs: i64, nanos: u32, offset: Option<i32>, zone: Option<&str>) -> Row {
    let mut row = row.int("secs", secs).int("nanos", i64::from(nanos));
    if let Some(offset) = offset {
        row = row.int("offset", i64::from(offset));
    }
    if let Some(zone) = zone {
        row = row.text("zone", zone);
    }
    leap(row, nanos)
}

/// Marks a value that holds a leap second.
fn leap(row: Row, nanos: u32) -> Row {
    if nanos >= NANOS {
        row.flag("leap", true)
    } else {
        row
    }
}

impl Value for DateTime<Utc> {
    const TYPE: &'static str = "DateTime<Utc>";

    fn canon(&self) -> String {
        instant_text(self.timestamp(), self.timestamp_subsec_nanos(), None, None)
    }

    fn parts(&self, row: Row) -> Row {
        instant_parts(
            row,
            self.timestamp(),
            self.timestamp_subsec_nanos(),
            None,
            None,
        )
    }
}

impl Value for DateTime<FixedOffset> {
    const TYPE: &'static str = "DateTime<FixedOffset>";

    fn canon(&self) -> String {
        let offset = self.offset().local_minus_utc();
        instant_text(
            self.timestamp(),
            self.timestamp_subsec_nanos(),
            Some(offset),
            None,
        )
    }

    fn parts(&self, row: Row) -> Row {
        let offset = self.offset().local_minus_utc();
        instant_parts(
            row,
            self.timestamp(),
            self.timestamp_subsec_nanos(),
            Some(offset),
            None,
        )
    }
}

impl Value for DateTime<Tz> {
    const TYPE: &'static str = "DateTime<Tz>";

    fn canon(&self) -> String {
        let offset = self.offset().fix().local_minus_utc();
        instant_text(
            self.timestamp(),
            self.timestamp_subsec_nanos(),
            Some(offset),
            Some(self.timezone().name()),
        )
    }

    fn parts(&self, row: Row) -> Row {
        let offset = self.offset().fix().local_minus_utc();
        instant_parts(
            row,
            self.timestamp(),
            self.timestamp_subsec_nanos(),
            Some(offset),
            Some(self.timezone().name()),
        )
    }
}

impl Value for NaiveDate {
    const TYPE: &'static str = "NaiveDate";

    fn canon(&self) -> String {
        date_text(i64::from(self.year()), self.month(), self.day())
    }

    fn parts(&self, row: Row) -> Row {
        row.int("year", i64::from(self.year()))
            .int("month", i64::from(self.month()))
            .int("day", i64::from(self.day()))
    }
}

impl Value for NaiveTime {
    const TYPE: &'static str = "NaiveTime";

    fn canon(&self) -> String {
        time_text(
            i64::from(self.hour()),
            i64::from(self.minute()),
            i64::from(self.second()),
            self.nanosecond(),
        )
    }

    fn parts(&self, row: Row) -> Row {
        let row = row
            .int("hour", i64::from(self.hour()))
            .int("minute", i64::from(self.minute()))
            .int("second", i64::from(self.second()))
            .int("nanos", i64::from(self.nanosecond()));
        leap(row, self.nanosecond())
    }
}

impl Value for NaiveDateTime {
    const TYPE: &'static str = "NaiveDateTime";

    fn canon(&self) -> String {
        format!("{}T{}", self.date().canon(), self.time().canon())
    }

    fn parts(&self, row: Row) -> Row {
        self.time().parts(self.date().parts(row))
    }
}

/// The whole seconds (rounded down) and the nanoseconds above them (0 to
/// 999,999,999) of a span: chrono's own two fields, which its JSON writes.
fn delta_fields(delta: &TimeDelta) -> (i64, i64) {
    let total =
        i128::from(delta.num_seconds()) * i128::from(NANOS) + i128::from(delta.subsec_nanos());
    (
        total.div_euclid(i128::from(NANOS)) as i64,
        total.rem_euclid(i128::from(NANOS)) as i64,
    )
}

impl Value for TimeDelta {
    const TYPE: &'static str = "TimeDelta";

    /// Seconds as a signed decimal with nine digits: `-0.500000000s`.
    fn canon(&self) -> String {
        let total =
            i128::from(self.num_seconds()) * i128::from(NANOS) + i128::from(self.subsec_nanos());
        let sign = if total < 0 { "-" } else { "" };
        let abs = total.unsigned_abs();
        format!(
            "{sign}{}.{:09}s",
            abs / u128::from(NANOS),
            abs % u128::from(NANOS)
        )
    }

    fn parts(&self, row: Row) -> Row {
        let (secs, nanos) = delta_fields(self);
        row.int("secs", secs).int("nanos", nanos)
    }
}

impl Value for Weekday {
    const TYPE: &'static str = "Weekday";

    fn canon(&self) -> String {
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][self.num_days_from_monday() as usize]
            .to_owned()
    }

    fn parts(&self, row: Row) -> Row {
        row.int("from_monday", i64::from(self.num_days_from_monday()))
    }
}

impl Value for Month {
    const TYPE: &'static str = "Month";

    fn canon(&self) -> String {
        self.name().to_owned()
    }

    fn parts(&self, row: Row) -> Row {
        row.int("number", i64::from(self.number_from_month()))
    }
}

impl Value for FixedOffset {
    const TYPE: &'static str = "FixedOffset";

    fn canon(&self) -> String {
        offset_text(self.local_minus_utc())
    }

    fn parts(&self, row: Row) -> Row {
        row.int("offset", i64::from(self.local_minus_utc()))
    }
}

// ---------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------

/// Year, month, day, hour, minute, second, nanosecond.
type Civil = (i32, u32, u32, u32, u32, u32, u32);

fn naive((year, month, day, hour, minute, second, nanos): Civil) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_nano_opt(hour, minute, second, nanos))
        .unwrap_or_else(|| {
            panic!("{year}-{month}-{day} {hour}:{minute}:{second}.{nanos} is not in the calendar")
        })
}

fn east(seconds: i32) -> FixedOffset {
    FixedOffset::east_opt(seconds).unwrap_or_else(|| panic!("{seconds} is not an offset"))
}

fn zone(name: &str) -> Tz {
    Tz::parse(name).unwrap_or_else(|_| panic!("{name} is not a zone"))
}

/// Keeps the first of each value.
fn distinct<T: PartialEq + Copy>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut kept: Vec<T> = Vec::new();
    for value in values {
        if !kept.contains(&value) {
            kept.push(value);
        }
    }
    kept
}

/// The values every form is recorded for, as dates and times without a
/// zone: the edges of everything a form can depend on.
const CORE: &[Civil] = &[
    // The epoch and the instants beside it.
    (1970, 1, 1, 0, 0, 0, 0),
    (1969, 12, 31, 23, 59, 59, 999_999_999),
    (1970, 1, 1, 0, 0, 0, 1),
    (1969, 12, 31, 23, 59, 59, 0),
    (1970, 1, 1, 0, 0, 1, 0),
    (1970, 1, 2, 0, 0, 0, 0),
    // The days the docs use.
    (2026, 1, 15, 12, 0, 0, 0),
    (2026, 11, 25, 9, 30, 0, 0),
    // Fractions of one to nine digits.
    (2026, 1, 15, 12, 0, 0, 100_000_000),
    (2026, 1, 15, 12, 0, 0, 120_000_000),
    (2026, 1, 15, 12, 0, 0, 123_000_000),
    (2026, 1, 15, 12, 0, 0, 123_400_000),
    (2026, 1, 15, 12, 0, 0, 123_450_000),
    (2026, 1, 15, 12, 0, 0, 123_456_000),
    (2026, 1, 15, 12, 0, 0, 123_456_700),
    (2026, 1, 15, 12, 0, 0, 123_456_780),
    (2026, 1, 15, 12, 0, 0, 123_456_789),
    // The smallest and largest fractions, and ones with zeros in front.
    (2026, 1, 15, 12, 0, 0, 1),
    (2026, 1, 15, 12, 0, 0, 1_000),
    (2026, 1, 15, 12, 0, 0, 1_000_000),
    (2026, 1, 15, 12, 0, 0, 7_000),
    (2026, 1, 15, 12, 0, 0, 26_490_000),
    (2026, 1, 15, 12, 0, 0, 50_000_000),
    (2026, 1, 15, 12, 0, 0, 500_000_000),
    (2026, 1, 15, 12, 0, 0, 999_000_000),
    (2026, 1, 15, 12, 0, 0, 999_999_000),
    (2026, 1, 15, 12, 0, 0, 999_999_999),
    // 29 February, and the years that have none.
    (1900, 2, 28, 12, 0, 0, 0),
    (1900, 3, 1, 0, 0, 0, 0),
    (2000, 2, 29, 0, 0, 0, 0),
    (2024, 2, 29, 23, 59, 59, 0),
    (2100, 2, 28, 23, 59, 59, 0),
    (2100, 3, 1, 0, 0, 0, 0),
    // Years at the edges of four digits, and before year 1.
    (0, 1, 1, 0, 0, 0, 0),
    (0, 12, 31, 23, 59, 59, 0),
    (1, 1, 1, 0, 0, 0, 0),
    (5, 6, 7, 8, 9, 10, 0),
    (99, 12, 31, 23, 59, 59, 0),
    (100, 1, 1, 0, 0, 0, 0),
    (999, 12, 31, 23, 59, 59, 0),
    (1000, 1, 1, 0, 0, 0, 0),
    (9999, 12, 31, 23, 59, 59, 0),
    (9999, 12, 31, 23, 59, 59, 999_999_999),
    (10000, 1, 1, 0, 0, 0, 0),
    (25599, 12, 31, 23, 59, 59, 0),
    (25600, 1, 1, 0, 0, 0, 0),
    (99999, 12, 31, 12, 0, 0, 0),
    (-1, 12, 31, 23, 59, 59, 0),
    (-1, 1, 1, 0, 0, 0, 0),
    (-44, 3, 15, 12, 0, 0, 0),
    (-99, 7, 4, 12, 0, 0, 0),
    (-100, 1, 1, 0, 0, 0, 0),
    (-9999, 1, 1, 0, 0, 0, 0),
    (-10000, 12, 31, 23, 59, 59, 0),
    (1582, 10, 4, 12, 0, 0, 0),
    (1582, 10, 15, 12, 0, 0, 0),
    // The 12-hour clock.
    (2026, 7, 4, 0, 0, 0, 0),
    (2026, 7, 4, 0, 30, 0, 0),
    (2026, 7, 4, 1, 0, 0, 0),
    (2026, 7, 4, 9, 5, 3, 0),
    (2026, 7, 4, 11, 59, 59, 0),
    (2026, 7, 4, 12, 0, 0, 0),
    (2026, 7, 4, 12, 30, 0, 0),
    (2026, 7, 4, 13, 0, 0, 0),
    (2026, 7, 4, 23, 59, 59, 0),
    // Weeks at the turn of a year.
    (2016, 1, 3, 10, 11, 12, 0),
    (2017, 1, 1, 10, 11, 12, 0),
    (2018, 12, 31, 10, 11, 12, 0),
    (2020, 12, 31, 10, 11, 12, 0),
    (2021, 1, 1, 10, 11, 12, 0),
    (2021, 1, 3, 10, 11, 12, 0),
    (2021, 1, 4, 10, 11, 12, 0),
    (2023, 1, 1, 10, 11, 12, 0),
    (2024, 1, 1, 10, 11, 12, 0),
    (2024, 12, 29, 10, 11, 12, 0),
    (2024, 12, 30, 10, 11, 12, 0),
    (2024, 12, 31, 10, 11, 12, 0),
    (2026, 1, 1, 10, 11, 12, 0),
    (2026, 12, 31, 10, 11, 12, 0),
    (2027, 1, 3, 10, 11, 12, 0),
    (2027, 1, 4, 10, 11, 12, 0),
    // Seconds since the epoch that are round numbers or the ends of 32 bits.
    (1901, 12, 13, 20, 45, 52, 0),
    (1973, 3, 3, 9, 46, 40, 0),
    (2001, 9, 9, 1, 46, 40, 0),
    (2038, 1, 19, 3, 14, 7, 0),
    (2038, 1, 19, 3, 14, 8, 0),
    (2286, 11, 20, 17, 46, 40, 0),
];

/// Values that hold a leap second, which chrono keeps as nanoseconds of one
/// second or more in second 59.
const LEAP: &[Civil] = &[
    (2016, 12, 31, 23, 59, 59, 1_000_000_000),
    (2015, 6, 30, 23, 59, 59, 1_500_000_000),
    (2001, 7, 8, 0, 34, 59, 1_026_490_000),
    (2026, 1, 15, 12, 30, 59, 1_999_999_999),
];

/// The values each of the longer patterns is recorded for: a morning, a
/// fraction, a leap day's last second, half past midnight, a year of one
/// digit, the last nanosecond of year 9999, and a year before 1.
const FEW: &[Civil] = &[
    (2026, 11, 25, 9, 30, 0, 0),
    (2026, 1, 15, 12, 0, 0, 123_456_789),
    (2024, 2, 29, 23, 59, 59, 0),
    (2026, 7, 4, 0, 30, 0, 500_000_000),
    (5, 6, 7, 8, 9, 10, 0),
    (9999, 12, 31, 23, 59, 59, 999_999_999),
    (-1, 12, 31, 23, 59, 59, 0),
];

fn few_naive() -> Vec<NaiveDateTime> {
    FEW.iter().copied().map(naive).collect()
}

/// The values each specifier of the vocabulary is recorded for, alone and
/// with each padding modifier.
const GENERAL: &[Civil] = &[
    (1970, 1, 1, 0, 0, 0, 0),
    (2026, 1, 15, 12, 0, 0, 0),
    (2026, 11, 25, 9, 30, 0, 0),
    (2026, 1, 15, 12, 0, 0, 123_456_789),
    (2026, 1, 15, 12, 0, 0, 500_000_000),
    (2026, 1, 15, 12, 0, 0, 7_000),
    (2024, 2, 29, 23, 59, 59, 0),
    (2026, 7, 4, 0, 30, 0, 0),
    (2026, 7, 4, 13, 5, 9, 0),
    (2021, 1, 1, 10, 11, 12, 0),
    (2001, 9, 9, 1, 46, 40, 0),
    (0, 1, 1, 0, 0, 0, 0),
    (5, 6, 7, 8, 9, 10, 0),
    (-1, 12, 31, 23, 59, 59, 0),
    (9999, 12, 31, 23, 59, 59, 999_999_999),
    (10000, 1, 1, 0, 0, 0, 0),
];

fn core_naive() -> Vec<NaiveDateTime> {
    let mut values = vec![NaiveDateTime::MAX, NaiveDateTime::MIN];
    values.extend(CORE.iter().copied().map(naive));
    // One week, day by day, and the middle of each month.
    values.extend((23..=29).map(|day| naive((2026, 11, day, 10, 11, 12, 0))));
    values.extend((1..=12).map(|month| naive((2026, month, 15, 18, 45, 0, 0))));
    distinct(values)
}

fn leap_naive() -> Vec<NaiveDateTime> {
    LEAP.iter().copied().map(naive).collect()
}

fn general_naive() -> Vec<NaiveDateTime> {
    let mut values = vec![NaiveDateTime::MAX, NaiveDateTime::MIN];
    values.extend(GENERAL.iter().copied().map(naive));
    values
}

fn utc(values: &[NaiveDateTime]) -> Vec<DateTime<Utc>> {
    values.iter().map(NaiveDateTime::and_utc).collect()
}

/// The last day of every month from 1969 to 2101.
fn month_ends() -> Vec<NaiveDate> {
    let mut days = Vec::new();
    for year in 1969..=2101 {
        for month in 1..=12 {
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let last = NaiveDate::from_ymd_opt(next_year, next_month, 1)
                .and_then(|first| first.pred_opt())
                .expect("a month has a last day");
            days.push(last);
        }
    }
    days
}

/// The times of day the month ends take in turn (13 of them, so a month
/// meets every one over the years): hour, minute, second, nanosecond.
const SWEEP_TIMES: [(u32, u32, u32, u32); 13] = [
    (0, 0, 0, 0),
    (23, 59, 59, 0),
    (12, 0, 0, 0),
    (9, 30, 0, 500_000_000),
    (17, 30, 0, 123_000_000),
    (6, 7, 8, 123_456_000),
    (18, 19, 20, 123_456_789),
    (23, 59, 59, 999_999_999),
    (0, 0, 0, 1),
    (13, 14, 15, 100_000_000),
    (1, 2, 3, 120_000_000),
    (21, 43, 5, 123_000),
    (4, 5, 6, 123_400_000),
];

/// The offsets the month ends take in turn, in seconds east.
const SWEEP_OFFSETS: [i32; 11] = [
    19_800, -28_800, 0, 50_400, -43_200, 20_700, -12_600, 45_900, 3_600, -18_000, 34_200,
];

/// Every month end, at its time of day.
fn sweep_naive() -> Vec<NaiveDateTime> {
    month_ends()
        .iter()
        .enumerate()
        .map(|(i, day)| {
            let (hour, minute, second, nanos) = SWEEP_TIMES[i % SWEEP_TIMES.len()];
            day.and_hms_nano_opt(hour, minute, second, nanos)
                .expect("a time of day")
        })
        .collect()
}

/// Every month end, at its time of day on a clock with its offset.
fn sweep_fixed() -> Vec<DateTime<FixedOffset>> {
    sweep_naive()
        .iter()
        .enumerate()
        .map(|(i, local)| {
            east(SWEEP_OFFSETS[i % SWEEP_OFFSETS.len()])
                .from_local_datetime(local)
                .single()
                .expect("a fixed offset has one instant for a local time")
        })
        .collect()
}

/// Offsets at the edges of what an offset can be and of how one is written:
/// whole and half hours, seconds that round up or down to a minute, the
/// local mean times of the zones below, and the widest allowed.
const EDGE_OFFSETS: &[i32] = &[
    0, 1, -1, 29, 30, -29, -30, 59, 60, -60, 1_800, 3_600, -3_600, 19_800, 20_700, 21_208, -17_762,
    -75, 34_200, 35_999, 45_900, 50_400, -43_200, 86_340, -86_340, 86_399, -86_399,
];

fn core_fixed() -> Vec<DateTime<FixedOffset>> {
    let sample = naive((2026, 1, 15, 12, 0, 0, 123_456_789)).and_utc();
    let mut values = Vec::new();
    for &offset in EDGE_OFFSETS {
        values.push(sample.with_timezone(&east(offset)));
        values.push(DateTime::UNIX_EPOCH.with_timezone(&east(offset)));
    }
    // The ends of the range, where a local time can lie outside it, and
    // years whose local year differs from the UTC one.
    let edges = [
        DateTime::<Utc>::MAX_UTC,
        DateTime::<Utc>::MIN_UTC,
        naive((0, 1, 1, 0, 0, 0, 0)).and_utc(),
        naive((9999, 12, 31, 23, 59, 59, 0)).and_utc(),
        naive((2024, 2, 29, 23, 59, 59, 500_000_000)).and_utc(),
        naive((2016, 12, 31, 23, 59, 59, 1_000_000_000)).and_utc(),
    ];
    for at in edges {
        for offset in [19_800, -28_800, 60, -60, 86_399, -86_399] {
            values.push(at.with_timezone(&east(offset)));
        }
    }
    // Two instants are equal to chrono whatever their offsets, so nothing
    // here is compared: every pair above differs in instant or in offset.
    values
}

fn general_fixed() -> Vec<DateTime<FixedOffset>> {
    let sample = naive((2026, 1, 15, 12, 0, 0, 123_456_789)).and_utc();
    let mut values: Vec<DateTime<FixedOffset>> = [
        19_800, -28_800, 0, 21_208, -17_762, 30, -30, 86_399, -86_399,
    ]
    .iter()
    .map(|&offset| sample.with_timezone(&east(offset)))
    .collect();
    values.push(
        naive((2026, 7, 4, 18, 45, 0, 0))
            .and_utc()
            .with_timezone(&east(20_700)),
    );
    values.push(DateTime::<Utc>::MAX_UTC.with_timezone(&east(60)));
    values.push(DateTime::<Utc>::MAX_UTC.with_timezone(&east(-28_800)));
    values.push(DateTime::<Utc>::MIN_UTC.with_timezone(&east(-60)));
    values.push(DateTime::<Utc>::MIN_UTC.with_timezone(&east(19_800)));
    values.push(
        naive((0, 1, 1, 0, 0, 0, 0))
            .and_utc()
            .with_timezone(&east(-3_600)),
    );
    values.push(
        naive((9999, 12, 31, 23, 59, 59, 0))
            .and_utc()
            .with_timezone(&east(3_600)),
    );
    values
}

/// The zones whose daylight-saving changes are swept: one with none, the
/// common one-hour change in both hemispheres' styles, a half-hour change, a
/// 45-minute offset, and a change at local midnight.
const ZONES: [&str; 6] = [
    "Asia/Kolkata",
    "America/New_York",
    "Europe/London",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "America/Havana",
];

/// More zones for the values every form is recorded for and for local times:
/// two more whose clocks skip midnight, one whose winter time is the
/// adjusted one, one that left daylight saving, one on a half hour.
const MORE_ZONES: [&str; 5] = [
    "America/Santiago",
    "Asia/Beirut",
    "Europe/Dublin",
    "America/Sao_Paulo",
    "America/St_Johns",
];

fn secs_of(year: i64, month: u32, day: u32) -> i64 {
    days_from_civil(year, month, day) * 86_400
}

fn at_secs(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap_or_else(|| panic!("{secs} is out of range"))
}

/// The offset (seconds east) and abbreviation in force in `tz` at `secs`.
fn zone_state(tz: Tz, secs: i64) -> (i32, &'static str) {
    let offset = tz.offset_from_utc_datetime(&at_secs(secs).naive_utc());
    (offset.fix().local_minus_utc(), offset.abbreviation())
}

/// Every instant from 1800 to 2040 at which the zone's offset or
/// abbreviation changes, to the second: the first second of the new one.
fn transitions(tz: Tz) -> Vec<i64> {
    let end = secs_of(2040, 1, 1);
    let mut found = Vec::new();
    let mut at = secs_of(1800, 1, 1);
    let mut state = zone_state(tz, at);
    while at < end {
        let next = at + 86_400;
        if zone_state(tz, next) == state {
            at = next;
            continue;
        }
        // `state` holds at `low` and no longer at `high`.
        let (mut low, mut high) = (at, next);
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if zone_state(tz, middle) == state {
                low = middle;
            } else {
                high = middle;
            }
        }
        found.push(high);
        at = high;
        state = zone_state(tz, high);
    }
    found
}

fn year_of(secs: i64) -> i64 {
    civil_from_days(secs.div_euclid(86_400)).0
}

/// The values every form is recorded for, in a zone: a winter and a summer
/// day, a day when the zone kept local mean time, each side of every change
/// in 2026, and the ends of the range.
fn core_zoned() -> Vec<DateTime<Tz>> {
    let mut values = Vec::new();
    for name in ZONES.iter().chain(&MORE_ZONES) {
        let tz = zone(name);
        values.push(
            naive((2026, 1, 15, 12, 0, 0, 0))
                .and_utc()
                .with_timezone(&tz),
        );
        values.push(
            naive((2026, 7, 15, 12, 0, 0, 123_456_789))
                .and_utc()
                .with_timezone(&tz),
        );
        values.push(naive((1850, 6, 1, 0, 0, 0, 0)).and_utc().with_timezone(&tz));
        for change in transitions(tz) {
            if year_of(change) == 2026 {
                values.push(at_secs(change - 1).with_timezone(&tz));
                values.push(at_secs(change).with_timezone(&tz));
            }
        }
    }
    for name in ["Asia/Kolkata", "America/New_York", "Pacific/Chatham"] {
        let tz = zone(name);
        values.push(DateTime::UNIX_EPOCH.with_timezone(&tz));
        values.push(DateTime::<Utc>::MAX_UTC.with_timezone(&tz));
        values.push(DateTime::<Utc>::MIN_UTC.with_timezone(&tz));
    }
    values
}

/// The zoned values each specifier is recorded for: an afternoon in India,
/// each side of New York's first change of 2026, the first second of Lord
/// Howe's and Havana's, Chatham's 45 minutes, Dublin's winter, a local mean
/// time with seconds in its offset, and the ends of the range.
fn general_zoned() -> Vec<DateTime<Tz>> {
    let sample = naive((2026, 1, 15, 12, 0, 0, 123_456_789)).and_utc();
    let first_change = |tz: Tz| {
        transitions(tz)
            .into_iter()
            .find(|change| year_of(*change) == 2026)
            .unwrap_or_else(|| panic!("{} has no change in 2026", tz.name()))
    };
    let kolkata = zone("Asia/Kolkata");
    let new_york = zone("America/New_York");
    let change = first_change(new_york);
    vec![
        sample.with_timezone(&kolkata),
        at_secs(change - 1).with_timezone(&new_york),
        at_secs(change).with_timezone(&new_york),
        at_secs(first_change(zone("Australia/Lord_Howe")))
            .with_timezone(&zone("Australia/Lord_Howe")),
        at_secs(first_change(zone("America/Havana"))).with_timezone(&zone("America/Havana")),
        sample.with_timezone(&zone("Pacific/Chatham")),
        sample.with_timezone(&zone("Europe/Dublin")),
        naive((1850, 6, 1, 0, 0, 0, 0))
            .and_utc()
            .with_timezone(&kolkata),
        DateTime::<Utc>::MAX_UTC.with_timezone(&kolkata),
        DateTime::<Utc>::MIN_UTC.with_timezone(&new_york),
    ]
}

/// The second before and the first second of every change of the six zones.
fn sweep_zoned() -> Vec<DateTime<Tz>> {
    let mut values = Vec::new();
    for name in ZONES {
        let tz = zone(name);
        for change in transitions(tz) {
            values.push(at_secs(change - 1).with_timezone(&tz));
            values.push(at_secs(change).with_timezone(&tz));
        }
    }
    values
}

fn core_dates() -> Vec<NaiveDate> {
    let mut values = vec![NaiveDate::MAX, NaiveDate::MIN];
    values.extend(core_naive().iter().map(NaiveDateTime::date));
    // 1 January and 31 December of years that start on each day of the
    // week, the quarters' first days, and day 366.
    for year in [2015, 2016, 2017, 2018, 2019, 2020, 2022] {
        values.push(naive((year, 1, 1, 0, 0, 0, 0)).date());
        values.push(naive((year, 12, 31, 0, 0, 0, 0)).date());
    }
    for (month, day) in [(3, 31), (4, 1), (6, 30), (7, 1), (9, 30), (10, 1)] {
        values.push(naive((2026, month, day, 0, 0, 0, 0)).date());
    }
    distinct(values)
}

fn core_times() -> Vec<NaiveTime> {
    let mut values: Vec<NaiveTime> = core_naive().iter().map(NaiveDateTime::time).collect();
    values.extend(SWEEP_TIMES.iter().map(|&(hour, minute, second, nanos)| {
        NaiveTime::from_hms_nano_opt(hour, minute, second, nanos).expect("a time of day")
    }));
    distinct(values)
}

fn leap_times() -> Vec<NaiveTime> {
    distinct(leap_naive().iter().map(NaiveDateTime::time))
}

/// Spans, as chrono's whole seconds (rounded down) and nanoseconds above.
const DELTAS: &[(i64, u32)] = &[
    (0, 0),
    (0, 1),
    (-1, 999_999_999),
    (0, 1_000),
    (0, 1_000_000),
    (0, 100_000_000),
    (0, 120_000_000),
    (0, 123_000_000),
    (0, 123_400_000),
    (0, 123_450_000),
    (0, 123_456_000),
    (0, 123_456_700),
    (0, 123_456_780),
    (0, 123_456_789),
    (0, 500_000_000),
    (-1, 500_000_000),
    (1, 0),
    (-1, 0),
    (1, 500_000_000),
    (-2, 500_000_000),
    (59, 0),
    (60, 0),
    (-60, 0),
    (90, 0),
    (3_600, 0),
    (-3_600, 0),
    (5_400, 0),
    (86_399, 999_999_999),
    (86_400, 0),
    (-86_400, 0),
    (604_800, 0),
    (2_592_000, 0),
    (31_536_000, 0),
    (-31_536_000, 0),
    (1_000_000_000, 0),
    (2_147_483_647, 0),
    (-2_147_483_648, 0),
    (1_000_000_000_000, 7),
    (8_210_266_876_799, 999_999_999),
    (-8_334_601_228_800, 0),
];

fn deltas() -> Vec<TimeDelta> {
    let mut values: Vec<TimeDelta> = DELTAS
        .iter()
        .map(|&(secs, nanos)| {
            TimeDelta::new(secs, nanos).unwrap_or_else(|| panic!("{secs}s {nanos}ns is no span"))
        })
        .collect();
    values.push(TimeDelta::MAX);
    values.push(TimeDelta::MIN);
    values
}

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

fn months() -> Vec<Month> {
    (1u8..=12)
        .map(|number| Month::try_from(number).expect("a month"))
        .collect()
}

fn offsets() -> Vec<FixedOffset> {
    EDGE_OFFSETS.iter().map(|&offset| east(offset)).collect()
}

// ---------------------------------------------------------------------------
// Display, Debug, RFC 3339, RFC 2822, JSON
// ---------------------------------------------------------------------------

/// `Display` and `Debug` of each value.
fn shown<V: Value + Display + Debug>(g: &mut Golden, values: &[V]) {
    for value in values {
        let display = written(|text| write!(text, "{value}"));
        let debug = written(|text| write!(text, "{value:?}"));
        let row = value.row().outcome(&display);
        g.push(
            DISPLAY,
            match &debug {
                Ok(text) => row.text("debug", text),
                Err(why) => row.text("debug_err", why),
            },
        );
    }
}

/// One line for each format string: what `format!` with it writes for
/// `$value`.
macro_rules! specs {
    ($g:expr, $file:expr, $row:expr, $value:expr; $($spec:literal),+ $(,)?) => {
        $(
            $g.push(
                $file,
                $row.text("spec", $spec)
                    .outcome(&written(|text| write!(text, $spec, $value))),
            );
        )+
    };
}

/// What a width, a fill, a precision or a flag does to `Display` and
/// `Debug`.
fn padded<V: Value + Display + Debug>(g: &mut Golden, values: &[V]) {
    for value in values {
        specs!(
            g, DISPLAY, value.row(), value;
            "{:>40}", "{:<40}", "{:^40}", "{:*^40}", "{:040}", "{:.3}", "{:+}", "{:>40?}",
            "{:#?}",
        );
    }
}

const SECONDS: [(&str, SecondsFormat); 5] = [
    ("secs", SecondsFormat::Secs),
    ("millis", SecondsFormat::Millis),
    ("micros", SecondsFormat::Micros),
    ("nanos", SecondsFormat::Nanos),
    ("auto", SecondsFormat::AutoSi),
];

/// `to_rfc3339()` of each value and, with `every_option`,
/// `to_rfc3339_opts` for each way of writing the seconds, with and without
/// `Z`.
fn rfc3339<Z: TimeZone>(g: &mut Golden, values: &[DateTime<Z>], every_option: bool)
where
    DateTime<Z>: Value,
{
    for value in values {
        g.push(
            RFC3339,
            value
                .row()
                .text("call", "to_rfc3339")
                .outcome(&called(|| value.to_rfc3339())),
        );
        if !every_option {
            continue;
        }
        for (name, seconds) in SECONDS {
            for z in [true, false] {
                g.push(
                    RFC3339,
                    value
                        .row()
                        .text("call", "to_rfc3339_opts")
                        .text("seconds", name)
                        .flag("z", z)
                        .outcome(&called(|| value.to_rfc3339_opts(seconds, z))),
                );
            }
        }
    }
}

/// `to_rfc2822()` of each value.
fn rfc2822<Z: TimeZone>(g: &mut Golden, values: &[DateTime<Z>])
where
    DateTime<Z>: Value,
{
    for value in values {
        g.push(RFC2822, value.row().outcome(&called(|| value.to_rfc2822())));
    }
}

/// The JSON document `serde_json::to_string` writes for each value.
fn json<V: Value + Serialize>(g: &mut Golden, values: &[V]) {
    for value in values {
        let outcome = match guard(|| serde_json::to_string(value)) {
            Ok(Ok(document)) => Ok(document),
            Ok(Err(_)) => Err("format"),
            Err(()) => Err("panic"),
        };
        g.push(SERDE, value.row().outcome(&outcome));
    }
}

fn written_forms(g: &mut Golden) {
    let core = core_naive();
    let leaps = leap_naive();
    let sweep = sweep_naive();

    // Instants in UTC. chrono's largest one comes first: it is the value
    // "never expires" is stored as.
    let instants = [utc(&core), utc(&leaps)].concat();
    let swept = utc(&sweep);
    shown(g, &instants);
    shown(g, &swept);
    padded(g, &instants[..3]);
    rfc3339(g, &instants, true);
    rfc3339(g, &swept, false);
    rfc2822(g, &instants);
    rfc2822(g, &swept);
    json(g, &instants);
    json(g, &swept);

    // Instants with a fixed offset. The month ends are in `Display` and in
    // JSON; RFC 3339 and RFC 2822 write them as they write the UTC ones,
    // with the offsets the values above cover.
    let fixed = core_fixed();
    let swept = sweep_fixed();
    shown(g, &fixed);
    shown(g, &swept);
    padded(g, &fixed[..2]);
    rfc3339(g, &fixed, true);
    rfc2822(g, &fixed);
    json(g, &fixed);
    json(g, &swept);

    // Instants in a zone. Every change of the six zones is in `Display`,
    // with the offset and the abbreviation on each side of it.
    let zoned = core_zoned();
    shown(g, &zoned);
    shown(g, &sweep_zoned());
    padded(g, &zoned[..2]);
    rfc3339(g, &zoned, true);
    rfc2822(g, &zoned);
    json(g, &zoned);

    // Days, times of day, and the two together.
    let dates = core_dates();
    let swept = month_ends();
    shown(g, &dates);
    shown(g, &swept);
    padded(g, &dates[..3]);
    json(g, &dates);
    json(g, &swept);

    let times = [core_times(), leap_times()].concat();
    shown(g, &times);
    padded(g, &times[..2]);
    json(g, &times);

    let date_times = [core, leaps].concat();
    shown(g, &date_times);
    shown(g, &sweep);
    padded(g, &date_times[..3]);
    json(g, &date_times);
    json(g, &sweep);

    // Spans, days of the week, months, offsets.
    let spans = deltas();
    shown(g, &spans);
    padded(g, &spans[..3]);
    json(g, &spans);

    shown(g, &WEEKDAYS);
    padded(g, &WEEKDAYS[..1]);
    json(g, &WEEKDAYS);

    // A month has `Debug` and a name, no `Display`.
    for month in months() {
        g.push(
            DISPLAY,
            month
                .row()
                .text("name", month.name())
                .text("debug", &format!("{month:?}")),
        );
    }
    json(g, &months());

    let offsets = offsets();
    shown(g, &offsets);
    padded(g, &offsets[..1]);
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

/// The specifiers that write one number, which take a padding modifier.
const NUMERIC: &[&str] = &[
    "Y", "C", "y", "q", "m", "d", "e", "w", "u", "U", "W", "G", "g", "V", "j", "H", "k", "I", "l",
    "M", "S", "f", "s",
];

/// Every other specifier of the vocabulary.
const OTHER: &[&str] = &[
    "%b", "%B", "%h", "%a", "%A", "%D", "%x", "%F", "%v", "%P", "%p", "%.f", "%.3f", "%.6f",
    "%.9f", "%3f", "%6f", "%9f", "%R", "%T", "%X", "%r", "%Z", "%z", "%:z", "%::z", "%:::z", "%#z",
    "%c", "%+", "%t", "%n", "%%",
];

/// The patterns Navree's own code, docs and tests format or parse with.
const NAVREE: &[&str] = &[
    "%Y-%m-%d",
    "%H:%M",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M",
    "%H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%d %H:%M UTC",
    "%Y-%m-%dT%H:%M:%S",
    "%-d %b",
    "%-d %b %Y",
    "%Y-%m-%dT%H:%M:%SZ",
    "%Y%m%dT%H%M%SZ",
    "%Y-%m-%d %H:%M:%S%.f%:z",
    "%Y-%m",
    "%b %Y",
    "%Y-%m-%d %H:%M:%S%.f%#z",
    "%Y%m%d-%H%M%S",
    "%-d %B %Y",
    "%A, %-d %B",
    "%-d %b, %H:%M",
    "%A %-d %B",
    "%Y-%m-%dT%H:%M:%S%.6fZ",
    "%Y-%m-%d %H:%M:%S UTC",
    "%-d %b %Y %H:%M UTC",
    "%-d %b %Y %H:%M %Z",
    "%A, %d-%b-%y %H:%M:%S GMT",
    "%a %b %e %H:%M:%S %Y",
    "%H:%M:%S%.3f",
    "%Y/%m/%d",
    "%H:%M:%S%.6f",
    "%Y-%m-%dT%H:%M:%S+00:00",
    "%-d %b %Y, %H:%M UTC",
    "%Y-%m-%d %H:%M:%S%.f%z",
    "%Y-%m-%d %H:%M:%S%.f %:z",
    "%Y-%m-%dT%H:%M:%S%:z",
    "%d.%m.%Y",
    "%Y-%m-%d %H:%M %:z",
    "%Y-%m-01",
    "%Y-01-01",
    "%Y-%m-01 00:00:00",
    "%Y-01-01 00:00:00",
    "%Y-%m-%d %H:%M:%S%.3f",
    "%a, %d %b %Y %H:%M:%S GMT",
    "%Y-%m-%dT%H:%M:%S%.3fZ",
    "%Y-%m-%d %H:%M:%S %Z %:z",
    "%Y-%m-%d %H:%M:%S%.6f",
    "%d %H:%M",
    "%d %b %Y",
    "%d %b %Y %H:%M",
    "%H:%M %z",
    "%B %-d, %Y",
    "%A %-d %B %Y",
    "Due %-d %b",
    "%a %-d %b, %H:%M",
    "%-d %b %Y %H:%M",
    "%-d %b %H:%M",
    "%-d %B",
    "%d/%m/%Y",
    "%d/%m/%Y %H:%M",
];

/// The patterns SQLx's chrono support writes and reads database text with.
const SQLX: &[&str] = &[
    "%F",
    "%T%.f",
    "%F %T%.f",
    "%F %R",
    "%F %RZ",
    "%F %R%:z",
    "%F %T%.fZ",
    "%F %T%.f%:z",
    "%FT%R",
    "%FT%RZ",
    "%FT%R%:z",
    "%FT%T%.f",
    "%FT%T%.fZ",
    "%FT%T%.f%:z",
    "%T.f",
    "%RZ",
    "%T%.fZ",
    "%R%:z",
    "%T%.f%:z",
];

/// Patterns that put specifiers together in ways the lists above do not.
const MIXED: &[&str] = &[
    "%Y-%j",
    "%G-W%V-%u",
    "%Y-W%W-%w",
    "%Y-U%U-%a",
    "%C%y-%m-%d",
    "%y-%m-%d",
    "%I:%M:%S %p",
    "%l:%M %P",
    "%k:%M",
    "%e %B %Y",
    "%Y-%m-%d%n%H:%M%t%S",
    "100%% %Y",
    "%Y\u{5e74}%m\u{6708}%d\u{65e5}",
    "%Y-%m-%d\u{a0}%H:%M",
    "%s%.3f",
    "%Y-%m-%dT%H:%M:%S%.9f%::z",
    "%Y-%m-%d %H:%M:%S %:::z",
];

/// A numeric specifier alone and with each padding modifier.
fn modified(letter: &str) -> [String; 4] {
    [
        format!("%{letter}"),
        format!("%-{letter}"),
        format!("%_{letter}"),
        format!("%0{letter}"),
    ]
}

/// Each specifier of the vocabulary alone, the numeric ones with each
/// padding modifier too.
fn single_patterns() -> Vec<String> {
    let mut patterns: Vec<String> = NUMERIC.iter().flat_map(|letter| modified(letter)).collect();
    patterns.extend(OTHER.iter().map(|pattern| (*pattern).to_owned()));
    patterns
}

/// The longer patterns: Navree's, SQLx's and the mixed ones, without the
/// ones that are a single specifier.
fn longer_patterns() -> Vec<String> {
    let singles = single_patterns();
    let mut kept: Vec<String> = Vec::new();
    for list in [NAVREE, SQLX, MIXED] {
        for pattern in list {
            let pattern = (*pattern).to_owned();
            if !singles.contains(&pattern) && !kept.contains(&pattern) {
                kept.push(pattern);
            }
        }
    }
    kept
}

/// Whether chrono takes `pattern` (`StrftimeItems` yields no error item).
fn pattern_ok(pattern: &str) -> bool {
    StrftimeItems::new(pattern).parse().is_ok()
}

/// More patterns, most of them refused, to pin where the vocabulary ends.
const ODD_PATTERNS: &[&str] = &[
    "",
    " ",
    "%",
    "%%",
    "%%%",
    "%%%%",
    "% ",
    "%Y%",
    "%Y-%m-%d%",
    "%Y-%Q",
    "%Q",
    "%E",
    "%O",
    "%Ey",
    "%EY",
    "%Od",
    "%Om",
    "%N",
    "%L",
    "%J",
    "%i",
    "%o",
    "%K",
    "%-",
    "%_",
    "%0",
    "%#",
    "%:",
    "%::",
    "%:::",
    "%::::z",
    "%:::::z",
    "%:y",
    "%:Y",
    "%.",
    "%.3",
    "%.f3",
    "%.3g",
    "%.3F",
    "%.ff",
    "%--d",
    "%-_d",
    "%_-d",
    "%00d",
    "%-0d",
    "%10d",
    "%2d",
    "%02d",
    "%-2d",
    "%-%",
    "%_%",
    "%0%",
    "%-t",
    "%_n",
    "%#-z",
    "%-#z",
    "%#:z",
    "%:#z",
    "%-:z",
    "%0::z",
    "%-.f",
    "%-.3f",
    "%_3f",
    "%09f",
    "%\u{e9}",
    "%\u{1f600}",
    "%Y %\u{e9}",
    "%+%+",
    "%Z%z",
    "%z%Z",
    "%%Y",
    "%%%Y",
    "%Y%%",
    "%t%n",
    "\u{5e74}%Y",
    "%Y\u{1f600}%m",
    "%Y\t%m\n%d",
    "%Y\u{a0}%m",
    "%Y\u{2003}%m",
    "100%",
    "50%% off",
    "%d %b %Y %H:%M:%S %Z %z %:z %::z %:::z",
];

/// `patterns.jsonl`: which patterns chrono takes.
fn pattern_rows(g: &mut Golden, used: &[String]) {
    let mut patterns: Vec<String> = Vec::new();
    // Each letter, digit and mark after `%`, alone and after each modifier
    // and `#`.
    for c in '!'..='~' {
        for prefix in ["%", "%-", "%_", "%0", "%#"] {
            patterns.push(format!("{prefix}{c}"));
        }
    }
    // Each digit count before `f`, with and without the dot.
    for digits in 0..=9 {
        patterns.push(format!("%{digits}f"));
        patterns.push(format!("%.{digits}f"));
    }
    patterns.extend(ODD_PATTERNS.iter().map(|pattern| (*pattern).to_owned()));
    patterns.extend(used.iter().cloned());
    let mut seen = BTreeSet::new();
    for pattern in patterns {
        if !seen.insert(pattern.clone()) {
            continue;
        }
        let out = if pattern_ok(&pattern) {
            "ok"
        } else {
            "BadFormat"
        };
        g.push(
            PATTERNS,
            Row::of("pattern").text("value", &pattern).text("out", out),
        );
    }
}

/// The types `format(pattern)` is recorded for.
trait Strftime: Value {
    fn strftime(&self, pattern: &str) -> Outcome;
}

macro_rules! strftime {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl Strftime for $ty {
                fn strftime(&self, pattern: &str) -> Outcome {
                    written(|text| write!(text, "{}", self.format(pattern)))
                }
            }
        )+
    };
}

strftime!(
    DateTime<Utc>,
    DateTime<FixedOffset>,
    DateTime<Tz>,
    NaiveDate,
    NaiveTime,
    NaiveDateTime,
);

fn strftime_row<V: Strftime>(g: &mut Golden, value: &V, pattern: &str) {
    g.used(pattern);
    g.push(
        STRFTIME,
        value
            .row()
            .text("pattern", pattern)
            .outcome(&value.strftime(pattern)),
    );
}

/// Every value with every pattern its type can write, and `sample` alone
/// with the patterns it cannot (a day has no hour): those fail for every
/// value, which is checked here.
fn strftime_own<V: Strftime>(g: &mut Golden, sample: &V, values: &[V], patterns: &[String]) {
    let (own, foreign): (Vec<&String>, Vec<&String>) = patterns
        .iter()
        .partition(|pattern| sample.strftime(pattern).is_ok());
    for pattern in &foreign {
        strftime_row(g, sample, pattern);
    }
    for value in values {
        for pattern in &foreign {
            assert!(
                value.strftime(pattern).is_err(),
                "{pattern} fails for {} and not for {}",
                sample.canon(),
                value.canon()
            );
        }
        for pattern in &own {
            strftime_row(g, value, pattern);
        }
    }
}

/// One pattern for every value.
fn strftime_one<V: Strftime>(g: &mut Golden, values: &[V], pattern: &str) {
    for value in values {
        strftime_row(g, value, pattern);
    }
}

/// A pattern of every date specifier, for the month ends.
const EVERY_DATE: &str = "%Y %C %y %q %m %b %B %d %e %a %A %w %u %U %W %G %g %V %j %D %F %v";

/// The pattern navree-time's tests show a zoned instant with.
const ZONED: &str = "%Y-%m-%d %H:%M:%S %Z %:z";

/// `strftime.jsonl`. Each specifier, alone and with each modifier, meets
/// every edge it can depend on: on a day for the date specifiers, on a time
/// of day for the time ones, and on a date and time or an instant (where
/// all of them can be written) for a set of values that covers the range.
/// The longer patterns are recorded for fewer values.
fn strftime_rows(g: &mut Golden, singles: &[String], longer: &[String]) {
    let instant = ["%s", "%+", "%c"];
    let offset = ["%z", "%:z", "%::z", "%:::z", "%Z", "%+", "%s", "%c"];

    let few = utc(&few_naive());
    strftime_own(g, &few[0], &utc(&general_naive()), singles);
    strftime_own(g, &few[0], &few, longer);
    let core = [utc(&core_naive()), utc(&leap_naive())].concat();
    for pattern in instant {
        strftime_one(g, &core, pattern);
    }
    // `format(..)` is a value of its own, and a width or a precision
    // applies to all of what it writes.
    specs!(
        g, STRFTIME, few[0].row().text("pattern", "%Y-%m-%d"), few[0].format("%Y-%m-%d");
        "{:>16}", "{:<16}", "{:^16}", "{:*^16}", "{:.4}",
    );

    let general = general_fixed();
    strftime_own(g, &general[0], &general, singles);
    strftime_own(g, &general[0], &general[..5], longer);
    let fixed = core_fixed();
    for pattern in offset {
        strftime_one(g, &fixed, pattern);
    }

    let general = general_zoned();
    strftime_own(g, &general[0], &general, singles);
    strftime_own(g, &general[0], &general[..5], longer);
    let zoned = core_zoned();
    for pattern in offset {
        strftime_one(g, &zoned, pattern);
    }
    strftime_one(g, &zoned, ZONED);

    let few = few_naive();
    let leaps = leap_naive();
    strftime_own(
        g,
        &few[0],
        &[general_naive(), leaps.clone()].concat(),
        singles,
    );
    strftime_own(
        g,
        &few[0],
        &[few.clone(), leaps[..1].to_vec()].concat(),
        longer,
    );

    let few_dates: Vec<NaiveDate> = few_naive().iter().map(NaiveDateTime::date).collect();
    strftime_own(g, &few_dates[0], &core_dates(), singles);
    strftime_own(g, &few_dates[0], &few_dates, longer);
    strftime_one(g, &month_ends(), EVERY_DATE);

    let few_times: Vec<NaiveTime> = few_naive().iter().map(NaiveDateTime::time).collect();
    let times = [core_times(), leap_times()].concat();
    strftime_own(g, &few_times[0], &times, singles);
    strftime_own(
        g,
        &few_times[0],
        &[few_times.clone(), leap_times()].concat(),
        longer,
    );
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Texts for an instant: what is written today, every looser form, and
/// what must be refused.
const INSTANT_TEXT: &[&str] = &[
    // As written today.
    "2026-01-15T12:00:00Z",
    "2026-01-15T12:00:00+00:00",
    "2026-01-15T12:00:00-00:00",
    "2026-01-15T17:30:00+05:30",
    "2026-01-15T04:00:00-08:00",
    "2026-01-15T17:45:00+05:45",
    "2026-01-15T12:00:00.5Z",
    "2026-01-15T12:00:00.123Z",
    "2026-01-15T12:00:00.123456Z",
    "2026-01-15T12:00:00.123456789Z",
    "2026-01-15T12:00:00.123456789+05:30",
    "2026-01-15T12:00:00.000000000Z",
    "2026-01-15T12:00:00.1234567891Z",
    "2026-01-15T12:00:00.12345678999999Z",
    "2026-01-15T12:00:00.Z",
    "2026-01-15T12:00:00.",
    "2026-01-15T12:00:00.5",
    "1970-01-01T00:00:00Z",
    "0000-01-01T00:00:00Z",
    "0001-01-01T00:00:00Z",
    "9999-12-31T23:59:59Z",
    "9999-12-31T23:59:59.999999999Z",
    "9999-12-31T23:59:59-00:01",
    "0000-01-01T00:00:00+00:01",
    "2024-02-29T00:00:00Z",
    "2000-02-29T00:00:00Z",
    // Lower case, a space for the T, and a name for UTC.
    "2026-01-15t12:00:00z",
    "2026-01-15T12:00:00z",
    "2026-01-15t12:00:00Z",
    "2026-01-15 12:00:00Z",
    "2026-01-15 12:00:00+05:30",
    "2026-01-15 12:00:00 +05:30",
    "2026-01-15 12:00:00 Z",
    "2026-01-15 12:00:00 UTC",
    "2026-01-15 12:00:00 utc",
    "2026-01-15 12:00:00 Utc",
    "2026-01-15T12:00:00UTC",
    "2026-01-15T12:00:00 UTC",
    "2026-01-15 12:00:00.5 UTC",
    "2026-01-15 12:00:00 GMT",
    "2026-01-15 12:00:00 UT",
    "2026-01-15 12:00:00 IST",
    "2026-01-15 12:00:00 UTC+05:30",
    "2026-01-15  12:00:00Z",
    "2026-01-15\t12:00:00Z",
    "2026-01-15_12:00:00Z",
    "2026-01-15T12:00:00 +0530",
    // The fraction.
    "2026-01-15T12:00:00,5Z",
    "2026-01-15T12:00:00,123+05:30",
    "2026-01-15T12:00:00.5 Z",
    "2026-01-15T12:00:00 .5Z",
    "2026-01-15T12:00:00. 5Z",
    "2026-01-15T12:00:00.-5Z",
    "2026-01-15T12:00:00.5.5Z",
    "2026-01-15T12:00.5Z",
    // The offset.
    "2026-01-15T12:00:00+0530",
    "2026-01-15T12:00:00+05",
    "2026-01-15T12:00:00+5",
    "2026-01-15T12:00:00+5:30",
    "2026-01-15T12:00:00+05:3",
    "2026-01-15T12:00:00+05:",
    "2026-01-15T12:00:00+05:30:00",
    "2026-01-15T12:00:00+05:30:15",
    "2026-01-15T12:00:00+053000",
    "2026-01-15T12:00:00+05 30",
    "2026-01-15T12:00:00+05: 30",
    "2026-01-15T12:00:00+05 :30",
    "2026-01-15T12:00:00+05::30",
    "2026-01-15T12:00:00\u{2212}05:30",
    "2026-01-15T12:00:00\u{2212}0530",
    "2026-01-15T12:00:00+00:00Z",
    "2026-01-15T12:00:00Z+00:00",
    "2026-01-15T12:00:00+",
    "2026-01-15T12:00:00-",
    "2026-01-15T12:00:00+23:59",
    "2026-01-15T12:00:00-23:59",
    "2026-01-15T12:00:00+24:00",
    "2026-01-15T12:00:00-24:00",
    "2026-01-15T12:00:00+25:00",
    "2026-01-15T12:00:00+99:59",
    "2026-01-15T12:00:00+00:60",
    "2026-01-15T12:00:00+05:99",
    "2026-01-15T12:00:00+2359",
    "2026-01-15T12:00:00+2400",
    // Parts left out or run together.
    "2026-01-15T12:00:00",
    "2026-01-15T12:00+05:30",
    "2026-01-15T12:00Z",
    "2026-01-15T12Z",
    "2026-01-15T",
    "2026-01-15Z",
    "20260115T120000Z",
    "2026-01-15T120000Z",
    "20260115T12:00:00Z",
    "2026-01-15T12:00:00Z junk",
    "2026-01-15T12:00:00Zz",
    "2026-01-15T12:00:00+05:30[Asia/Kolkata]",
    "2026-W03-4T12:00:00Z",
    "2026-015T12:00:00Z",
    // Fewer or more than two digits.
    "2026-1-15T12:00:00Z",
    "2026-01-5T12:00:00Z",
    "2026-1-5T1:2:3Z",
    "2026-01-15T1:02:03Z",
    "2026-01-15T12:0:0Z",
    "2026-001-15T12:00:00Z",
    "2026-01-015T12:00:00Z",
    "2026-01-15T012:00:00Z",
    "2026-01-15T12:000:00Z",
    "2026-01-15T12:00:000Z",
    "26-01-15T12:00:00Z",
    "026-01-15T12:00:00Z",
    "02026-01-15T12:00:00Z",
    "12345-01-15T12:00:00Z",
    // Years with a sign, and the ends of the range.
    "+2026-01-15T12:00:00Z",
    "-2026-01-15T12:00:00Z",
    "+02026-01-15T12:00:00Z",
    "+12345-01-15T12:00:00Z",
    "-0001-01-01T00:00:00Z",
    "-0000-01-01T00:00:00Z",
    "+0000-01-01T00:00:00Z",
    "+10000-01-01T00:00:00Z",
    "10000-01-01T00:00:00Z",
    "+262142-12-31T23:59:59.999999999Z",
    "+262142-12-31T23:59:59.999999999+00:00",
    "+262142-12-31T23:59:59.999999999-00:01",
    "+262142-12-31T23:59:59.999999999+00:01",
    "+262143-01-01T00:00:00Z",
    "+262143-01-01T00:59:59+01:00",
    "-262143-01-01T00:00:00Z",
    "-262143-01-01T00:00:00+00:01",
    "-262143-01-01T00:00:00-00:01",
    "-262144-12-31T23:00:00-01:00",
    "-262144-01-01T00:00:00Z",
    "+999999-01-01T00:00:00Z",
    "+99999999999-01-01T00:00:00Z",
    "+99999999999999999999-01-01T00:00:00Z",
    // White space.
    " 2026-01-15T12:00:00Z",
    "2026-01-15T12:00:00Z ",
    "\t2026-01-15T12:00:00Z\n",
    "\u{a0}2026-01-15T12:00:00Z",
    "2026 -01-15T12:00:00Z",
    "2026- 01-15T12:00:00Z",
    "2026-01 -15T12:00:00Z",
    "2026-01- 15T12:00:00Z",
    "2026-01-15 T12:00:00Z",
    "2026-01-15T 12:00:00Z",
    "2026-01-15T12 :00:00Z",
    "2026-01-15T12: 00:00Z",
    "2026-01-15T12:00 :00Z",
    "2026-01-15T12:00: 00Z",
    "2026-01-15T12:00:00 Z",
    "2026-  01-15T12:  00:00Z",
    // Other separators and other digits.
    "2026/01/15T12:00:00Z",
    "2026.01.15T12:00:00Z",
    "2026-01-15T12.00.00Z",
    "2026-01-15T12-00-00Z",
    "2026\u{2013}01\u{2013}15T12:00:00Z",
    "\u{ff12}\u{ff10}\u{ff12}\u{ff16}-01-15T12:00:00Z",
    "\u{662}\u{660}\u{662}\u{666}-01-15T12:00:00Z",
    // Days and times that do not exist.
    "2026-00-15T12:00:00Z",
    "2026-13-15T12:00:00Z",
    "2026-01-00T12:00:00Z",
    "2026-01-32T12:00:00Z",
    "2026-02-29T12:00:00Z",
    "2026-02-30T12:00:00Z",
    "1900-02-29T00:00:00Z",
    "2100-02-29T00:00:00Z",
    "2026-04-31T00:00:00Z",
    "2026-01-15T24:00:00Z",
    "2026-01-15T25:00:00Z",
    "2026-01-15T23:60:00Z",
    "2026-01-15T23:59:61Z",
    "2026-01-15T99:99:99Z",
    // Second 60.
    "2026-01-15T23:59:60Z",
    "2026-01-15T23:59:60.5Z",
    "2026-01-15T23:59:60.999999999Z",
    "2026-01-15T12:30:60Z",
    "2026-01-15T00:00:60+05:30",
    "2016-12-31T23:59:60+00:00",
    "2026-06-30T23:59:60-08:00",
    "9999-12-31T23:59:60Z",
    "+262142-12-31T23:59:60Z",
    // Not this kind of text at all.
    "",
    " ",
    "T",
    "Z",
    "now",
    "today",
    "1768478400",
    "1768478400.5",
    "Thu, 15 Jan 2026 12:00:00 +0000",
    "15 Jan 2026 12:00:00",
    "January 15, 2026",
];

/// Texts for a date and time without a zone.
const DATETIME_TEXT: &[&str] = &[
    "2026-01-15T09:30:00",
    "2026-01-15T09:30:05.5",
    "2026-01-15T09:30:05.123456789",
    "2026-01-15T09:30:05.1234567891",
    "2026-01-15T09:30:05.",
    "2026-01-15T09:30:05,5",
    "2026-01-15T9:30:0",
    "2026-1-5T9:3:0",
    "2026-01-15t09:30:00",
    "2026-01-15 09:30:00",
    "2026-01-15 09:30:00.5",
    "2026-01-15T09:30",
    "2026-01-15 09:30",
    "2026-01-15T09",
    "2026-01-15T09:30:00 ",
    " 2026-01-15T09:30:00",
    "2026-01-15 T09:30:00",
    "2026-01-15T 09:30:00",
    "2026-01-15  T  09:30:00",
    "2026-01-15T09 : 30 : 00",
    "2026-01-15T09:30:00 .5",
    "2026 - 01 - 15T09:30:00",
    "+12345-6-7T7:59:60.5",
    "-0044-03-15T12:00:00",
    "+262142-12-31T23:59:59.999999999",
    "+262143-01-01T00:00:00",
    "-262143-01-01T00:00:00",
    "-262144-12-31T23:59:59",
    "2026-01-15T23:59:60",
    "2026-01-15T23:59:60.999999999",
    "2026-01-15T24:00:00",
    "2026-02-29T09:30:00",
    "2024-02-29T09:30:00",
    "2026-01-15T09:30:00junk",
    "2026-01-15T09:30:00 junk",
    "20260115T093000",
    "2026-01-15T093000",
];

/// Texts for a day.
const DATE_TEXT: &[&str] = &[
    "2026-01-15",
    "2026-1-5",
    "2026-01-5",
    "2026-1-15",
    "2026-11-25",
    "2024-02-29",
    "2000-02-29",
    "2026-02-28",
    "2026-02-29",
    "1900-02-29",
    "2026-12-31",
    "0000-01-01",
    "0001-01-01",
    "9999-12-31",
    "1-1-1",
    "26-01-15",
    "026-01-15",
    "02026-01-15",
    "12345-06-07",
    "+2026-01-15",
    "+12345-6-7",
    "+012345-06-07",
    "-0001-01-01",
    "-1-1-1",
    "+262142-12-31",
    "+262143-01-01",
    "-262143-01-01",
    "-262144-12-31",
    "+99999999999-01-01",
    "99999999999999999999-01-01",
    " 2026-01-15",
    "2026-01-15 ",
    "2026-01-15\n",
    "\t2026-01-15",
    "2026 - 01 - 15",
    "2026- 01- 15",
    "2026 -01 -15",
    "2026-01-15 junk",
    "2026-01-15junk",
    "2026/01/15",
    "2026.01.15",
    "20260115",
    "2026-0115",
    "2026-01",
    "2026",
    "2026-01-",
    "2026--15",
    "-01-15",
    "2026-001-15",
    "2026-01-015",
    "2026-011-5",
    "2026-00-10",
    "2026-13-01",
    "2026-01-00",
    "2026-01-32",
    "2026-04-31",
    "2026-06-31",
    "15-01-2026",
    "15/01/2026",
    "01/15/2026",
    "Jan 15 2026",
    "2026-Jan-15",
    "2026\u{2013}01\u{2013}15",
    "\u{ff12}\u{ff10}\u{ff12}\u{ff16}-01-15",
    "-",
    "--",
    "abc",
];

/// Texts for a time of day.
const TIME_TEXT: &[&str] = &[
    "09:30:00",
    "9:30:00",
    "9:3:0",
    "09:30",
    "9:30",
    "0:0",
    "0:0:0",
    "00:00:00",
    "23:59:59",
    "09:30:05.5",
    "09:30:05.05",
    "09:30:05.123",
    "09:30:05.123456",
    "09:30:05.123456789",
    "09:30:05.1234567891",
    "09:30:05.000000000",
    "09:30:05.",
    "09:30:05,5",
    "09:30.5",
    "23:59:60",
    "23:59:60.5",
    "23:59:60.999999999",
    "12:30:60",
    "24:00:00",
    "24:00",
    "23:60:00",
    "23:60",
    "23:59:61",
    "99:99:99",
    "09:30:00 ",
    " 09:30:00",
    "09:30 ",
    "09 :30:00",
    "09: 30:00",
    "09:30 :00",
    "09:30: 00",
    "09 : 30 : 00",
    "09:30:00 .5",
    "09:30:00. 5",
    "09:30:00Z",
    "09:30:00+05:30",
    "09:30:00 UTC",
    "09:30 junk",
    "09:30:junk",
    "0930",
    "093000",
    "09",
    "9",
    "09:",
    ":30",
    "09:30:",
    "09::30",
    "09.30.00",
    "09-30-00",
    "9:30 AM",
    "09:30:00 AM",
    "9:30pm",
    "T09:30:00",
    "009:30:00",
    "09:030:00",
    "09:30:000",
    "09:30:5",
    "09:3",
    "-09:30:00",
    "+09:30:00",
    "noon",
    "midnight",
];

/// Texts for an offset.
const OFFSET_TEXT: &[&str] = &[
    "+05:30",
    "+0530",
    "-08:00",
    "-0800",
    "+00:00",
    "-00:00",
    "+0000",
    "-0000",
    "Z",
    "z",
    "UTC",
    "utc",
    "GMT",
    "+05",
    "-08",
    "+5",
    "+530",
    "+05:3",
    "+05:",
    "+05:30:00",
    "+05:30:15",
    "+053000",
    "+053015",
    "+05 30",
    "+05  30",
    "+05: 30",
    "+05 :30",
    "+05::30",
    "+05\t30",
    "+ 05:30",
    "+05.30",
    "+23:59",
    "-23:59",
    "+24:00",
    "-24:00",
    "+99:00",
    "+99:59",
    "+00:60",
    "+05:60",
    "+05:99",
    "\u{2212}05:30",
    "\u{2212}0530",
    "\u{2013}05:30",
    "05:30",
    "0530",
    "+",
    "-",
    "",
    "+0",
    "+a5:30",
    "+05:3a",
    "+05:30junk",
    "+05:30 ",
    " +05:30",
];

/// Texts for a day of the week.
const WEEKDAY_TEXT: &[&str] = &[
    "Mon",
    "Tue",
    "Wed",
    "Thu",
    "Fri",
    "Sat",
    "Sun",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "mon",
    "MON",
    "mOn",
    "monday",
    "MONDAY",
    "MoNdAy",
    "Mo",
    "M",
    "Mond",
    "Mondays",
    "Tues",
    "Wednes",
    "Thur",
    "Thurs",
    "Satur",
    " Mon",
    "Mon ",
    "Mon.",
    "Monday!",
    "1",
    "0",
    "",
    "Lun",
    "Somvar",
];

/// Texts for a month.
const MONTH_TEXT: &[&str] = &[
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
    "January",
    "February",
    "March",
    "April",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "jan",
    "JAN",
    "january",
    "JANUARY",
    "may",
    "MAY",
    "Sept",
    "Janu",
    "Januar",
    "Ja",
    " Jan",
    "Jan ",
    "Jan.",
    "1",
    "01",
    "",
    "Magh",
];

/// Texts in the form of RFC 2822 and of the HTTP dates beside it.
const RFC2822_TEXT: &[&str] = &[
    "Thu, 15 Jan 2026 12:00:00 +0000",
    "Thu, 15 Jan 2026 12:00:00 -0000",
    "Thu, 15 Jan 2026 12:00:00 GMT",
    "Thu, 15 Jan 2026 12:00:00 UT",
    "Thu, 15 Jan 2026 12:00:00 Z",
    "Thu, 15 Jan 2026 12:00:00 z",
    "Thu, 15 Jan 2026 12:00:00 gmt",
    "Thu, 15 Jan 2026 12:00:00 UTC",
    "Thu, 15 Jan 2026 17:30:00 +0530",
    "Thu, 15 Jan 2026 04:00:00 -0800",
    "Thu, 15 Jan 2026 07:00:00 EST",
    "Thu, 15 Jan 2026 08:00:00 EDT",
    "Thu, 15 Jan 2026 06:00:00 CST",
    "Thu, 15 Jan 2026 07:00:00 CDT",
    "Thu, 15 Jan 2026 05:00:00 MST",
    "Thu, 15 Jan 2026 06:00:00 MDT",
    "Thu, 15 Jan 2026 04:00:00 PST",
    "Thu, 15 Jan 2026 05:00:00 PDT",
    "Thu, 15 Jan 2026 07:00:00 est",
    "Thu, 15 Jan 2026 12:00:00 A",
    "Thu, 15 Jan 2026 12:00:00 J",
    "Thu, 15 Jan 2026 12:00:00 N",
    "Thu, 15 Jan 2026 12:00:00 y",
    "Thu, 15 Jan 2026 17:30:00 IST",
    "Thu, 15 Jan 2026 12:00:00 +05:30",
    "Thu, 15 Jan 2026 12:00:00 +530",
    "Thu, 15 Jan 2026 12:00:00 +05",
    "Thu, 15 Jan 2026 12:00:00 +2359",
    "Thu, 15 Jan 2026 12:00:00 +2400",
    "Thu, 15 Jan 2026 12:00:00 +9959",
    "Thu, 15 Jan 2026 12:00:00 -9959",
    "Thu, 15 Jan 2026 12:00:00 +0060",
    "Thu, 15 Jan 2026 12:00:00 \u{2212}0500",
    "Thu, 15 Jan 2026 12:00:00+0000",
    "Thu, 15 Jan 2026 12:00:00",
    "Thu, 15 Jan 2026 12:00:00 ",
    "15 Jan 2026 12:00:00 +0000",
    "Thu, 15 Jan 2026 12:00 +0000",
    "Thu, 15 Jan 2026 12 +0000",
    "Mon, 5 Jan 2026 12:00:00 +0000",
    "Mon, 05 Jan 2026 12:00:00 +0000",
    "Mon,  5 Jan 2026 12:00:00 +0000",
    "Mon, 005 Jan 2026 12:00:00 +0000",
    "Fri, 15 Jan 2026 12:00:00 +0000",
    "thu, 15 jan 2026 12:00:00 +0000",
    "THU, 15 JAN 2026 12:00:00 +0000",
    "Thursday, 15 Jan 2026 12:00:00 +0000",
    "Thu, 15 January 2026 12:00:00 +0000",
    "Thu 15 Jan 2026 12:00:00 +0000",
    "Thu,15 Jan 2026 12:00:00 +0000",
    "Thu , 15 Jan 2026 12:00:00 +0000",
    "Thu, 15Jan 2026 12:00:00 +0000",
    "Thu, 15 Jan2026 12:00:00 +0000",
    "Thu, 15 Jan 202612:00:00 +0000",
    "Thu, 15 Jan 26 12:00:00 +0000",
    "Fri, 15 Jan 49 12:00:00 +0000",
    "Sun, 15 Jan 50 12:00:00 +0000",
    "Fri, 15 Jan 99 12:00:00 +0000",
    "Thu, 15 Jan 126 12:00:00 +0000",
    "Fri, 15 Jan 009 12:00:00 +0000",
    "Thu, 15 Jan 0026 12:00:00 +0000",
    "Thu, 15 Jan 6 12:00:00 +0000",
    "Sat, 01 Jan 0000 00:00:00 +0000",
    "Fri, 31 Dec 9999 23:59:59 +0000",
    "Sat, 01 Jan 10000 00:00:00 +0000",
    "Thu, 1 Jan 1970 00:00:00 +0000",
    "Tue, 29 Feb 2000 00:00:00 +0000",
    "Sun, 29 Feb 2026 00:00:00 +0000",
    "Thu, 32 Jan 2026 12:00:00 +0000",
    "Thu, 0 Jan 2026 12:00:00 +0000",
    "Thu, 15 Jan 2026 24:00:00 +0000",
    "Thu, 15 Jan 2026 12:60:00 +0000",
    "Thu, 15 Jan 2026 12:00:60 +0000",
    "Thu, 15 Jan 2026 23:59:60 +0000",
    "Thu, 15 Jan 2026 12:00:61 +0000",
    "Thu, 15 Jan 2026 1:02:03 +0000",
    "Thu, 15 Jan 2026 12:0:0 +0000",
    "Thu, 15 Jan 2026 12 : 00 : 00 +0000",
    "Thu, 15 Jan 2026 12:00:00.5 +0000",
    "  Thu, 15 Jan 2026 12:00:00 +0000",
    "Thu, 15 Jan 2026 12:00:00 +0000  ",
    "Thu,\t15\tJan\t2026\t12:00:00\t+0000",
    "Thu, 15 Jan 2026 12:00:00 +0000 (IST)",
    "Thu, 15 Jan 2026 12:00:00 +0000 (a (nested) one)",
    "Thu, 15 Jan 2026 12:00:00 +0000 (one) (two)",
    "Thu, 15 Jan 2026 12:00:00 +0000 (an escaped \\) one)",
    "Thu, 15 Jan 2026 12:00:00 +0000 (never closed",
    "Thu, 15 Jan 2026 12:00:00 (first) +0000",
    "Thu, 15 Jan 2026 12:00:00 +0000 trailing",
    "Thu, 15 Jan 2026 12:00:00 GMT+1",
    "Sun, 06 Nov 1994 08:49:37 GMT",
    "Sunday, 06-Nov-94 08:49:37 GMT",
    "Sun Nov  6 08:49:37 1994",
    "2026-01-15T12:00:00Z",
    "2026-01-15 12:00:00 +0000",
    "",
    " ",
    "Thu",
    "Thu,",
];

/// JSON documents that are not a text, given to every type.
const JSON_OTHER: &[&str] = &[
    "null",
    "true",
    "0",
    "1768478400",
    "1768478400.5",
    "1768478400000",
    "[]",
    "{}",
    "[\"2026-01-15T12:00:00Z\"]",
    "{\"$date\":\"2026-01-15T12:00:00Z\"}",
    "",
    "2026-01-15",
    "\"2026-01-15T12:00:00Z\" ",
    " \"2026-01-15T12:00:00Z\"",
    "\"2026-01-15T12:00:00Z\"x",
];

/// JSON documents for a span.
const DELTA_JSON: &[&str] = &[
    "[0,0]",
    "[1,0]",
    "[1,500000000]",
    "[-1,500000000]",
    "[-1,0]",
    "[0,999999999]",
    "[0,1000000000]",
    "[0,-1]",
    "[1,-500000000]",
    "[9223372036854775,807000000]",
    "[9223372036854775,807000001]",
    "[9223372036854776,0]",
    "[-9223372036854776,193000000]",
    "[-9223372036854776,192999999]",
    "[-9223372036854777,0]",
    "[9223372036854775807,0]",
    "[9223372036854775808,0]",
    "[0,2147483647]",
    "[0,2147483648]",
    "[1]",
    "[]",
    "[1,2,3]",
    "[1.0,0]",
    "[1,0.0]",
    "[1.5,0]",
    "[\"1\",\"0\"]",
    "[null,0]",
    " [ 1 , 0 ] ",
    "{\"secs\":1,\"nanos\":0}",
    "1",
    "1.5",
    "\"PT1S\"",
    "\"1s\"",
    "null",
];

/// Patterns with texts written by hand for them: each text goes through
/// `parse_from_str` of an instant, a date and time, a day and a time of day.
const PATTERN_TEXT: &[(&str, &[&str])] = &[
    (
        "%Y-%m-%d",
        &[
            "2026-01-15",
            "2026-1-5",
            " 2026-01-15",
            "2026-01-15 ",
            "2026- 01-15",
            "2026 -01-15",
            "2026-01- 15",
            "+2026-01-15",
            "-0044-03-15",
            "02026-01-15",
            "12345-01-15",
            "+12345-01-15",
            "26-01-15",
            "2026-13-01",
            "2026-00-01",
            "2026-02-30",
            "1900-02-29",
            "2024-02-29",
            "2026-01-15T12:00:00",
            "20260115",
            "2026/01/15",
            "",
        ],
    ),
    (
        "%Y%m%d",
        &[
            "20260115",
            "2026115",
            "202601150",
            "+20260115",
            "0020260115",
            "2026 01 15",
        ],
    ),
    (
        "%Y %m %d",
        &[
            "2026 01 15",
            "20260115",
            "2026\t01\n15",
            "2026  1   5",
            "2026-01-15",
        ],
    ),
    (
        "%d/%m/%Y",
        &[
            "15/01/2026",
            "5/1/2026",
            "15/01/26",
            "31/04/2026",
            "29/02/2026",
            "29/02/2024",
            "15-01-2026",
            "15/01/2026 10:00",
            "15/01/+12345",
        ],
    ),
    (
        "%d/%m/%Y %H:%M",
        &[
            "15/01/2026 09:30",
            "15/01/2026  09:30",
            "15/01/202609:30",
            "15/01/2026\t09:30",
            "15/01/2026\u{a0}09:30",
            "15/01/2026 9:30",
            "15/01/2026 24:00",
            "15/01/2026 09:60",
            "15/01/2026",
            "15/01/2026 09:30:00",
        ],
    ),
    ("%d.%m.%Y", &["15.01.2026", "2026-02-01", "1.2.2026"]),
    (
        "%y-%m-%d",
        &[
            "26-01-15",
            "69-01-15",
            "70-01-15",
            "99-12-31",
            "00-01-01",
            "6-01-15",
            "126-01-15",
            "2026-01-15",
        ],
    ),
    (
        "%C%y-%m-%d",
        &[
            "2026-01-15",
            "1999-12-31",
            "0005-06-07",
            "026-01-15",
            "99999-01-01",
        ],
    ),
    ("%C-%m-%d", &["20-01-15"]),
    (
        "%Y-%j",
        &[
            "2026-015",
            "2026-15",
            "2026-1",
            "2024-366",
            "2026-366",
            "2026-000",
            "2026-0015",
        ],
    ),
    (
        "%G-W%V-%u",
        &[
            "2026-W03-4",
            "2020-W53-5",
            "2021-W53-1",
            "2025-W01-1",
            "2026-W00-1",
            "2026-W54-1",
            "2026-W03-0",
            "2026-W03-8",
        ],
    ),
    ("%g-W%V-%u", &["26-W03-4", "70-W01-1", "69-W01-1"]),
    (
        "%Y-W%W-%w",
        &[
            "2026-W02-4",
            "2026-W00-4",
            "2026-W00-1",
            "2026-W53-1",
            "2026-W02-7",
        ],
    ),
    (
        "%Y-U%U-%a",
        &["2026-U02-Thu", "2026-U00-Thu", "2026-U00-Sun"],
    ),
    ("%Y-%U", &["2026-02"]),
    ("%Y-%m-%d %u", &["2026-01-15 4", "2026-01-15 5"]),
    (
        "%Y-%m-%d %q",
        &[
            "2026-01-15 1",
            "2026-01-15 2",
            "2026-01-15 0",
            "2026-01-15 5",
        ],
    ),
    (
        "%Y-%m-%d %a",
        &[
            "2026-01-15 Thu",
            "2026-01-15 thu",
            "2026-01-15 THU",
            "2026-01-15 Fri",
            "2026-01-15 Thursday",
            "2026-01-15 Th",
        ],
    ),
    (
        "%A, %Y-%m-%d",
        &[
            "Thursday, 2026-01-15",
            "Thu, 2026-01-15",
            "thursday, 2026-01-15",
            "THURSDAY, 2026-01-15",
            "Thurs, 2026-01-15",
            "Friday, 2026-01-15",
        ],
    ),
    (
        "%d %b %Y",
        &[
            "15 Jan 2026",
            "15 jan 2026",
            "15 JAN 2026",
            "15 January 2026",
            "15 Ja 2026",
            "15 Sept 2026",
            "15 Sep 2026",
            "5 Jan 2026",
            "15  Jan  2026",
            "15Jan2026",
            "15 Jan",
        ],
    ),
    (
        "%d %B %Y",
        &[
            "15 January 2026",
            "15 Jan 2026",
            "15 january 2026",
            "15 JANUARY 2026",
            "15 Janu 2026",
            "15 May 2026",
            "15 Sept 2026",
            "15 September 2026",
            "15 Septembre 2026",
        ],
    ),
    ("%h %d %Y", &["Jan 15 2026"]),
    (
        "%B %-d, %Y",
        &[
            "January 15, 2026",
            "January 5, 2026",
            "January 05, 2026",
            "Jan 5, 2026",
        ],
    ),
    (
        "%-d %b %Y",
        &["5 Jan 2026", "05 Jan 2026", " 5 Jan 2026", "15 Jan 2026"],
    ),
    ("%e %b %Y", &[" 5 Jan 2026", "5 Jan 2026", "05 Jan 2026"]),
    ("%_d %b %Y", &[" 5 Jan 2026", "5 Jan 2026"]),
    (
        "%Y-%m-%d %H:%M:%S",
        &[
            "2026-01-15 09:30:05",
            "2026-01-15T09:30:05",
            "2026-01-15  09:30:05",
            "2026-01-1509:30:05",
            "2026-01-15 9:30:5",
            "2026-01-15 09:30",
            "2026-01-15 09:30:05.5",
            "2026-01-15 23:59:60",
            "2026-01-15 12:30:60",
            "2026-01-15 24:00:00",
            "2026-01-15 09:30:05 ",
            "2026-01-15 09:30:05Z",
            "2026-01-15",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S%.f",
        &[
            "2026-01-15 09:30:05",
            "2026-01-15 09:30:05.5",
            "2026-01-15 09:30:05.123456789",
            "2026-01-15 09:30:05.1234567891",
            "2026-01-15 09:30:05.",
            "2026-01-15 09:30:05,5",
            "2026-01-15 09:30:05 .5",
            "2026-01-15 23:59:60.5",
            "2026-01-15T09:30:05.5",
            "2026-01-15 09:30",
            "2026-01-15",
        ],
    ),
    (
        "%Y-%m-%dT%H:%M:%S%.f",
        &[
            "2026-01-15T09:30:05",
            "2026-01-15T09:30:05.5",
            "2026-01-15t09:30:05",
            "2026-01-15 09:30:05",
            "2026-01-15T09:30",
            "2026-01-15T09:30:05Z",
        ],
    ),
    (
        "%Y-%m-%d %H:%M",
        &[
            "2026-01-15 09:30",
            "2026-01-15 09:30:05",
            "2026-01-15 9:30",
            "2026-01-15T09:30",
        ],
    ),
    (
        "%Y-%m-%dT%H:%M",
        &[
            "2026-01-15T09:30",
            "2026-01-15T09:30:05",
            "2026-01-15 09:30",
        ],
    ),
    (
        "%Y-%m-%d %H:%M UTC",
        &[
            "2026-01-15 09:30 UTC",
            "2026-01-15 09:30 utc",
            "2026-01-15 09:30",
            "2026-01-15 09:30 UT",
        ],
    ),
    (
        "%H:%M",
        &[
            "09:30", "9:30", "09:30:00", "24:00", "23:59", "0:0", "09:3", "09:030", "09 :30",
            "09: 30", " 09:30", "09:30 ",
        ],
    ),
    (
        "%H:%M:%S",
        &[
            "09:30:05",
            "9:30:5",
            "09:30",
            "09:30:60",
            "09:30:61",
            "09:30:05.5",
        ],
    ),
    (
        "%H:%M:%S%.f",
        &[
            "09:30:05",
            "09:30:05.5",
            "09:30:05.123456789",
            "09:30:05.",
            "09:30:05,5",
            "09:30",
        ],
    ),
    (
        "%H:%M:%S%.3f",
        &[
            "09:30:05.123",
            "09:30:05.12",
            "09:30:05.1234",
            "09:30:05",
            "09:30:05.",
            "09:30:05.abc",
        ],
    ),
    (
        "%H:%M:%S%.6f",
        &[
            "09:30:05.123456",
            "09:30:05.123",
            "09:30:05.1234567",
            "09:30:05",
        ],
    ),
    (
        "%H:%M:%S%.9f",
        &[
            "09:30:05.123456789",
            "09:30:05.123",
            "09:30:05.1234567891",
            "09:30:05",
        ],
    ),
    (
        "%H:%M:%S.%3f",
        &["09:30:05.123", "09:30:05.12", "09:30:05.1234", "09:30:05."],
    ),
    ("%H%M%S%3f", &["093005123", "09300512"]),
    ("%S%6f", &["05123456", "0512345"]),
    ("%S%9f", &["05123456789", "0512345678"]),
    (
        "%H:%M:%S.%f",
        &[
            "09:30:05.5",
            "09:30:05.000000005",
            "09:30:05.123456789",
            "09:30:05.1234567891",
            "09:30:05.999999999",
            "09:30:05.",
        ],
    ),
    ("%.f", &[".5", "", "5"]),
    ("%H:%M%.f", &["09:30.5", "09:30"]),
    (
        "%I:%M %p",
        &[
            "09:30 AM",
            "09:30 PM",
            "9:30 am",
            "9:30 pM",
            "12:00 AM",
            "12:00 PM",
            "12:30 am",
            "00:30 AM",
            "13:00 PM",
            "09:30AM",
            "09:30 A",
            "09:30 a.m.",
            "09:30 XM",
            "09:30",
        ],
    ),
    ("%I:%M", &["09:30", "12:30"]),
    (
        "%H:%M %p",
        &["09:30 AM", "09:30 PM", "21:30 PM", "21:30 AM"],
    ),
    ("%l:%M %P", &[" 9:30 am", "9:30 pm", "12:30 AM"]),
    ("%k:%M", &[" 9:30", "9:30", "09:30", "23:59"]),
    ("%H", &["09", "9", "24"]),
    ("%M", &["30"]),
    ("%S", &["05"]),
    (
        "%H:%M:%S %Z",
        &[
            "09:30:05 IST",
            "09:30:05 UTC",
            "09:30:05 +05:30",
            "09:30:05 Asia/Kolkata",
            "09:30:05 ",
            "09:30:05",
            "09:30:05 IST junk",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S %Z",
        &[
            "2026-01-15 09:30:05 IST",
            "2026-01-15 09:30:05 UTC",
            "2026-01-15 09:30:05 +05:30",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S %Z %:z",
        &[
            "2026-01-15 09:30:05 IST +05:30",
            "2026-01-15 09:30:05 +05:30",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S%.f%:z",
        &[
            "2026-01-15 09:30:05+05:30",
            "2026-01-15 09:30:05.5+05:30",
            "2026-01-15 09:30:05.5 +05:30",
            "2026-01-15 09:30:05+0530",
            "2026-01-15 09:30:05Z",
            "2026-01-15 09:30:05",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S%.f%#z",
        &[
            "2026-01-15 09:30:05+00",
            "2026-01-15 09:30:05.123456+00",
            "2026-01-15 09:30:05.5+05:30",
            "2026-01-15 09:30:05+0530",
            "2026-01-15 09:30:05-08",
            "2026-01-15 09:30:05Z",
            "2026-01-15 09:30:05",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S%.f%z",
        &[
            "2026-01-15 09:30:05+0530",
            "2026-01-15 09:30:05.5+05:30",
            "2026-01-15 09:30:05Z",
        ],
    ),
    (
        "%Y-%m-%d %H:%M:%S%.f %:z",
        &[
            "2026-01-15 09:30:05 +05:30",
            "2026-01-15 09:30:05.5 +05:30",
            "2026-01-15 09:30:05+05:30",
        ],
    ),
    (
        "%Y-%m-%dT%H:%M:%S%:z",
        &[
            "2026-01-15T09:30:05+05:30",
            "2026-01-15T09:30:05Z",
            "2026-01-15T09:30:05.5+05:30",
        ],
    ),
    (
        "%Y-%m-%d %H:%M %:z",
        &["2026-01-15 09:30 +05:30", "2026-01-15 09:30"],
    ),
    (
        "%Y-%m-%dT%H:%M:%SZ",
        &[
            "2026-01-15T09:30:05Z",
            "2026-01-15T09:30:05z",
            "2026-01-15T09:30:05",
            "2026-01-15T09:30:05+00:00",
        ],
    ),
    (
        "%Y%m%dT%H%M%SZ",
        &[
            "20260115T093005Z",
            "20260115T093005z",
            "20260115T093005",
            "2026115T093005Z",
            "20260115T93005Z",
        ],
    ),
    (
        "%s",
        &[
            "0",
            "1768478400",
            "-1",
            "+1",
            " 42",
            "042",
            "1768478400.5",
            "8210266876799",
            "8210266876800",
            "99999999999999999999",
            "",
        ],
    ),
    ("%s %z", &["1768478400 +0530", "1768478400 +0000"]),
    ("%s%.3f", &["1768478400.123", "1768478400"]),
    (
        "%Y-%m-%d %s",
        &["2026-01-15 1768478400", "2026-01-16 1768478400"],
    ),
    (
        "%Y-%m-%d %H:%M:%S %s",
        &[
            "2026-01-15 12:00:00 1768478400",
            "2026-01-15 12:00:01 1768478400",
        ],
    ),
    (
        "%c",
        &[
            "Thu Jan 15 09:30:05 2026",
            "Mon Jan  5 09:30:05 2026",
            "Mon Jan 5 09:30:05 2026",
            "thu jan 15 09:30:05 2026",
            "Fri Jan 15 09:30:05 2026",
        ],
    ),
    (
        "%a %b %e %H:%M:%S %Y",
        &[
            "Sun Nov  6 08:49:37 1994",
            "Sun Nov 6 08:49:37 1994",
            "Sun Nov 06 08:49:37 1994",
            "Mon Nov  6 08:49:37 1994",
        ],
    ),
    (
        "%A, %d-%b-%y %H:%M:%S GMT",
        &[
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun, 06-Nov-94 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 UTC",
            "Sunday, 06-Nov-1994 08:49:37 GMT",
            "Sunday, 06-Nov-26 08:49:37 GMT",
            "Friday, 06-Nov-26 08:49:37 GMT",
        ],
    ),
    (
        "%a, %d %b %Y %H:%M:%S GMT",
        &[
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sun, 6 Nov 1994 08:49:37 GMT",
            "Sun, 06 Nov 1994 08:49:37 gmt",
            "Sunday, 06 Nov 1994 08:49:37 GMT",
        ],
    ),
    ("%D", &["01/15/26", "1/15/26", "01/15/2026", "13/15/26"]),
    ("%x", &["01/15/26"]),
    ("%F", &["2026-01-15", "+12345-01-15", "2026-1-5"]),
    ("%v", &["15-Jan-2026", " 5-Jan-2026", "5-Jan-2026"]),
    ("%T", &["09:30:05", "9:30:5", "09:30"]),
    ("%R", &["09:30", "9:30", "09:30:05"]),
    ("%X", &["09:30:05"]),
    (
        "%r",
        &[
            "09:30:05 AM",
            "09:30:05 PM",
            "12:00:00 AM",
            "09:30:05 am",
            "09:30:05AM",
            "09:30:05",
        ],
    ),
    ("%T.f", &["09:30:05.f", "09:30:05.5", "09:30:05"]),
    (
        "%Y-%m-%d%n%H:%M",
        &["2026-01-15\n09:30", "2026-01-15 09:30", "2026-01-1509:30"],
    ),
    (
        "%Y-%m-%d%t%H:%M",
        &["2026-01-15\t09:30", "2026-01-15 09:30"],
    ),
    ("%%%Y", &["%2026", "2026"]),
    ("100%% %Y", &["100% 2026", "100%% 2026"]),
    (
        "%Y\u{5e74}%m\u{6708}%d\u{65e5}",
        &[
            "2026\u{5e74}01\u{6708}15\u{65e5}",
            "2026\u{5e74}1\u{6708}5\u{65e5}",
            "2026-01-15",
        ],
    ),
    ("Due %-d %b", &["Due 15 Jan", "due 15 Jan", "Due 5 Jan"]),
    ("", &["", " ", "x"]),
    (" ", &["", " ", "   ", "\t\n", "\u{a0}", "x"]),
    ("%Y-%m-%d ", &["2026-01-15", "2026-01-15 ", "2026-01-15   "]),
    (" %Y-%m-%d", &["2026-01-15", "  2026-01-15"]),
    (
        "%Y-%m-%d\u{a0}%H:%M",
        &[
            "2026-01-15\u{a0}09:30",
            "2026-01-15 09:30",
            "2026-01-1509:30",
        ],
    ),
    ("%m/%d", &["01/15"]),
    ("%Y", &["2026", "+12345", "-1", "12345"]),
    ("%Y-%m", &["2026-01"]),
    ("%m", &["01"]),
    ("%d", &["15"]),
    ("%u", &["1", "7", "0", "8"]),
    ("%w", &["0", "6", "7"]),
    ("%j", &["015"]),
    ("%Y-%Q", &["2026-x"]),
    ("%Q", &["x", ""]),
    ("%", &["", "%"]),
    ("%-F", &["2026-01-15"]),
];

/// Why a text was refused, and the message when there is one to keep.
struct Refused {
    out: String,
    message: Option<String>,
}

impl From<chrono::ParseError> for Refused {
    /// chrono's kind of error: `Invalid`, `TooShort`, `OutOfRange`, ...
    fn from(error: chrono::ParseError) -> Self {
        Refused {
            out: format!("{:?}", error.kind()),
            message: None,
        }
    }
}

/// Records one text given to one way of reading: in `parse_accept.jsonl`
/// with the value it was read as, or in `parse_refuse.jsonl` with why not.
fn read<V: Value>(
    g: &mut Golden,
    via: &str,
    pattern: Option<&str>,
    input: &str,
    result: impl FnOnce() -> Result<V, Refused>,
) {
    let key = format!("{}\0{via}\0{}\0{input}", V::TYPE, pattern.unwrap_or("\0"));
    if !g.read.insert(key) {
        return;
    }
    let mut row = Row::of(V::TYPE).text("via", via);
    if let Some(pattern) = pattern {
        g.used(pattern);
        row = row.text("pattern", pattern);
    }
    let row = row.text("value", input);
    match guard(result) {
        Ok(Ok(value)) => {
            let row = value.parts(row.text("out", &value.canon()));
            g.push(PARSE_ACCEPT, row);
        }
        Ok(Err(refused)) => {
            let row = row.text("out", &refused.out);
            g.push(
                PARSE_REFUSE,
                match &refused.message {
                    Some(message) => row.text("message", message),
                    None => row,
                },
            );
        }
        Err(()) => g.push(PARSE_REFUSE, row.text("out", "panic")),
    }
}

/// `text.parse::<V>()` for each text.
fn from_str<V>(g: &mut Golden, texts: &[&str])
where
    V: Value + FromStr<Err = chrono::ParseError>,
{
    for text in texts {
        read::<V>(g, "from_str", None, text, || Ok(text.parse::<V>()?));
    }
}

/// chrono's messages for its kinds of error, as its `Display` writes them.
const KINDS: [(&str, &str); 7] = [
    ("input is out of range", "OutOfRange"),
    ("no possible date and time matching input", "Impossible"),
    ("input is not enough for unique date and time", "NotEnough"),
    ("input contains invalid characters", "Invalid"),
    ("premature end of input", "TooShort"),
    ("trailing input", "TooLong"),
    ("bad or unsupported format string", "BadFormat"),
];

/// `serde_json::from_str::<V>(document)` for each document. A document
/// that is a text and is refused by chrono has chrono's kind of error (its
/// message is the start of serde_json's); any other refusal is `serde`,
/// with serde_json's message.
fn from_json<V: Value + DeserializeOwned>(g: &mut Golden, documents: &[String]) {
    for document in documents {
        read::<V>(g, "serde", None, document, || {
            serde_json::from_str::<V>(document).map_err(|error| {
                let message = error.to_string();
                match KINDS.iter().find(|(text, _)| message.starts_with(text)) {
                    Some((_, kind)) => Refused {
                        out: (*kind).to_owned(),
                        message: None,
                    },
                    None => Refused {
                        out: "serde".to_owned(),
                        message: Some(message),
                    },
                }
            })
        });
    }
}

/// Each text as a JSON string, then the documents that are not a text.
fn documents(texts: &[&str]) -> Vec<String> {
    let mut documents: Vec<String> = texts
        .iter()
        .map(|text| {
            let mut document = String::new();
            json_text(&mut document, text);
            document
        })
        .collect();
    documents.extend(JSON_OTHER.iter().map(|document| (*document).to_owned()));
    documents
}

/// One text through `parse_from_str` of each of the four types.
fn from_pattern(g: &mut Golden, pattern: &str, text: &str) {
    let via = "pattern";
    read::<DateTime<FixedOffset>>(g, via, Some(pattern), text, || {
        Ok(DateTime::parse_from_str(text, pattern)?)
    });
    read::<NaiveDateTime>(g, via, Some(pattern), text, || {
        Ok(NaiveDateTime::parse_from_str(text, pattern)?)
    });
    read::<NaiveDate>(g, via, Some(pattern), text, || {
        Ok(NaiveDate::parse_from_str(text, pattern)?)
    });
    read::<NaiveTime>(g, via, Some(pattern), text, || {
        Ok(NaiveTime::parse_from_str(text, pattern)?)
    });
}

/// The instants whose text, written with each pattern, is read back with
/// it: an afternoon with a fraction, a leap day before midnight, a leap
/// second, a year of five digits, a year before 1, and two-digit years on
/// each side of where the century changes.
fn round_trip_values() -> Vec<DateTime<FixedOffset>> {
    let at = |civil: Civil, offset: i32| {
        east(offset)
            .from_local_datetime(&naive(civil))
            .single()
            .expect("a fixed offset has one instant for a local time")
    };
    vec![
        at((2026, 1, 15, 9, 30, 5, 123_456_789), 19_800),
        at((2024, 2, 29, 23, 59, 59, 0), -28_800),
        at((2001, 7, 8, 0, 34, 59, 1_026_490_000), 34_200),
        at((12345, 6, 7, 8, 9, 10, 500_000_000), 0),
        at((-44, 3, 15, 12, 0, 0, 0), 3_600),
        at((1969, 12, 31, 0, 0, 0, 0), 0),
        at((2070, 7, 4, 13, 5, 9, 0), -12_600),
    ]
}

fn reading(g: &mut Golden, patterns: &[String]) {
    let texts: Vec<&str> = [INSTANT_TEXT, DATETIME_TEXT, DATE_TEXT, TIME_TEXT].concat();

    // `FromStr` of each type, with every text.
    from_str::<DateTime<Utc>>(g, &texts);
    from_str::<DateTime<FixedOffset>>(g, &texts);
    from_str::<NaiveDateTime>(g, &texts);
    from_str::<NaiveDate>(g, &texts);
    from_str::<NaiveTime>(g, &texts);
    from_str::<FixedOffset>(g, OFFSET_TEXT);
    for text in WEEKDAY_TEXT {
        read::<Weekday>(g, "from_str", None, text, || {
            text.parse::<Weekday>().map_err(|_| Refused {
                out: "ParseWeekdayError".to_owned(),
                message: None,
            })
        });
    }
    for text in MONTH_TEXT {
        read::<Month>(g, "from_str", None, text, || {
            text.parse::<Month>().map_err(|_| Refused {
                out: "ParseMonthError".to_owned(),
                message: None,
            })
        });
    }

    // RFC 3339 and RFC 2822.
    for text in texts.iter().chain(RFC2822_TEXT) {
        read::<DateTime<FixedOffset>>(g, "rfc3339", None, text, || {
            Ok(DateTime::parse_from_rfc3339(text)?)
        });
    }
    for text in RFC2822_TEXT.iter().chain(INSTANT_TEXT.iter().take(12)) {
        read::<DateTime<FixedOffset>>(g, "rfc2822", None, text, || {
            Ok(DateTime::parse_from_rfc2822(text)?)
        });
    }

    // JSON.
    let json_texts = documents(&texts);
    from_json::<DateTime<Utc>>(g, &json_texts);
    from_json::<DateTime<FixedOffset>>(g, &json_texts);
    from_json::<NaiveDateTime>(g, &json_texts);
    from_json::<NaiveDate>(g, &json_texts);
    from_json::<NaiveTime>(g, &json_texts);
    from_json::<Weekday>(g, &documents(WEEKDAY_TEXT));
    from_json::<Month>(g, &documents(MONTH_TEXT));
    let spans: Vec<String> = DELTA_JSON.iter().map(|text| (*text).to_owned()).collect();
    from_json::<TimeDelta>(g, &spans);

    // Patterns: the texts written by hand for a pattern.
    for (pattern, pattern_texts) in PATTERN_TEXT {
        for text in *pattern_texts {
            from_pattern(g, pattern, text);
        }
    }
    // Every text for an instant through `%+`, which reads RFC 3339 loosely.
    for text in INSTANT_TEXT {
        from_pattern(g, "%+", text);
    }
    // Every text for an offset after each offset specifier.
    for specifier in ["%z", "%:z", "%::z", "%:::z", "%#z"] {
        let pattern = format!("%Y-%m-%d %H:%M:%S {specifier}");
        for offset in OFFSET_TEXT {
            from_pattern(g, &pattern, &format!("2026-01-15 09:30:05 {offset}"));
        }
        let pattern = format!("%Y-%m-%dT%H:%M:%S{specifier}");
        for offset in OFFSET_TEXT.iter().take(14) {
            from_pattern(g, &pattern, &format!("2026-01-15T09:30:05{offset}"));
        }
    }
    // Every pattern: what it writes for a value, read back with it.
    let values = round_trip_values();
    for pattern in patterns.iter().filter(|pattern| pattern_ok(pattern)) {
        for value in &values {
            if let Ok(text) = value.strftime(pattern) {
                from_pattern(g, pattern, &text);
            }
        }
    }

    // What a database holds as text, through every way such text is read.
    let stored = stored_texts();
    let stored: Vec<&str> = stored.iter().map(String::as_str).collect();
    from_str::<DateTime<Utc>>(g, &stored);
    from_str::<DateTime<FixedOffset>>(g, &stored);
    from_str::<NaiveDateTime>(g, &stored);
    from_str::<NaiveDate>(g, &stored);
    from_str::<NaiveTime>(g, &stored);
    for text in &stored {
        read::<DateTime<FixedOffset>>(g, "rfc3339", None, text, || {
            Ok(DateTime::parse_from_rfc3339(text)?)
        });
        for pattern in STORED_PATTERNS {
            from_pattern(g, pattern, text);
        }
    }
}

/// The patterns database text is read with today: navree-db's for a text
/// column, then SQLx's for SQLite (a date and time, then a time of day) and
/// for PostgreSQL's and MySQL's text format.
const STORED_PATTERNS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f%:z",
    "%Y-%m-%d %H:%M:%S%.f%#z",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%d",
    "%F %T%.f",
    "%F %R",
    "%F %RZ",
    "%F %R%:z",
    "%F %T%.fZ",
    "%F %T%.f%:z",
    "%FT%R",
    "%FT%RZ",
    "%FT%R%:z",
    "%FT%T%.f",
    "%FT%T%.fZ",
    "%FT%T%.f%:z",
    "%F",
    "%T.f",
    "%T%.f",
    "%R",
    "%RZ",
    "%T%.fZ",
    "%R%:z",
    "%T%.f%:z",
    "%H:%M:%S%.f",
];

/// What the databases hold as text for six instants (the largest, the
/// smallest, the epoch, a fraction, a year of one digit and one of five):
/// SQLite's text of the instant in UTC and at +05:30, of its date and time,
/// its day and its time of day, and the instant as PostgreSQL shows a
/// `timestamptz` in text. PostgreSQL writes a year above 9999 with no sign
/// and a year before 1 with `BC` after it, so those two are here by hand.
fn stored_texts() -> Vec<String> {
    let instants = [
        DateTime::<Utc>::MAX_UTC,
        DateTime::<Utc>::MIN_UTC,
        DateTime::UNIX_EPOCH,
        naive((2026, 1, 15, 12, 0, 0, 123_456_789)).and_utc(),
        naive((5, 6, 7, 8, 9, 10, 0)).and_utc(),
        naive((10000, 1, 1, 0, 0, 0, 0)).and_utc(),
    ];
    let mut texts = Vec::new();
    for at in instants {
        let micros = at
            .with_nanosecond(at.nanosecond() / 1_000 * 1_000)
            .expect("fewer nanoseconds are in range");
        texts.push(at.to_rfc3339());
        texts.push(at.with_timezone(&east(19_800)).to_rfc3339());
        texts.push(at.naive_utc().format("%F %T%.f").to_string());
        texts.push(at.naive_utc().format("%F").to_string());
        texts.push(at.naive_utc().format("%T%.f").to_string());
        texts.push(micros.format("%Y-%m-%d %H:%M:%S%.f+00").to_string());
    }
    texts.push("262142-12-31 23:59:59.999999+00".to_owned());
    texts.push("0044-03-15 12:00:00+00 BC".to_owned());
    texts.push("infinity".to_owned());
    texts.push("-infinity".to_owned());
    texts
}

// ---------------------------------------------------------------------------
// Local times in a zone
// ---------------------------------------------------------------------------

fn local_time_row(g: &mut Golden, tz: Tz, zone: &Zone, local: NaiveDateTime) {
    let row = local.parts(
        Row::of("LocalResult")
            .text("zone", zone.name())
            .text("value", &local.canon()),
    );
    let instant = |at: &DateTime<Tz>| (at.timestamp(), at.offset().fix().local_minus_utc());
    let row = match tz.from_local_datetime(&local) {
        LocalResult::None => row.text("out", "none"),
        LocalResult::Single(at) => {
            let (secs, offset) = instant(&at);
            row.text("out", "single")
                .int("secs", secs)
                .int("offset", i64::from(offset))
        }
        LocalResult::Ambiguous(earliest, latest) => {
            let (secs, offset) = instant(&earliest);
            let (later_secs, later_offset) = instant(&latest);
            row.text("out", "ambiguous")
                .int("secs", secs)
                .int("offset", i64::from(offset))
                .int("later_secs", later_secs)
                .int("later_offset", i64::from(later_offset))
        }
    };
    // What navree-time makes of it: a time that comes twice is the earlier
    // one, a time that is skipped moves forward by the length of the gap.
    g.push(LOCAL, row.int("to_utc", zone.to_utc(local).timestamp()));
}

fn local_day_row(g: &mut Golden, zone: &Zone, date: NaiveDate) {
    let day = LocalDay::from(date);
    let (start, end) = day.utc_bounds(zone);
    g.push(
        LOCAL,
        date.parts(
            Row::of("LocalDay")
                .text("zone", zone.name())
                .text("value", &date.canon()),
        )
        .int("start", start.timestamp())
        .int("end", end.timestamp()),
    );
}

/// `local.jsonl`: wall-clock times around each change of a zone's offset
/// (the second before the skipped or repeated span, its first second, its
/// middle, its last second and the second after it), and the first and last
/// instants of the days around the change.
fn local_times(g: &mut Golden) {
    for name in ZONES.iter().chain(&MORE_ZONES) {
        let tz = zone(name);
        let zone = Zone::parse(name).unwrap_or_else(|_| panic!("{name} is not a zone"));
        let mut walls = BTreeSet::new();
        let mut days = BTreeSet::new();
        for change in transitions(tz) {
            let year = year_of(change);
            let wanted =
                (1970..=1972).contains(&year) || (2016..=2030).contains(&year) || year >= 2037;
            let (before, _) = zone_state(tz, change - 1);
            let (after, _) = zone_state(tz, change);
            if !wanted || before == after {
                continue;
            }
            // The wall clock shows `low` to `high` twice, or not at all.
            let low = change + i64::from(before.min(after));
            let high = change + i64::from(before.max(after));
            walls.extend([low - 1, low, low + (high - low) / 2, high - 1, high]);
            for wall in [low, high] {
                let day = wall.div_euclid(86_400);
                days.extend([day - 1, day, day + 1]);
            }
        }
        // And two ordinary noons, so a zone with no change has lines too.
        for (month, day) in [(1, 15), (7, 15)] {
            let noon = secs_of(2026, month, day) + 43_200;
            walls.insert(noon);
            days.insert(noon.div_euclid(86_400));
        }
        for wall in walls {
            local_time_row(g, tz, &zone, at_secs(wall).naive_utc());
        }
        for day in days {
            local_day_row(g, &zone, at_secs(day * 86_400).date_naive());
        }
    }
}

// ---------------------------------------------------------------------------
// Databases
// ---------------------------------------------------------------------------

/// The names of the types among `infos` that `V` says it can be read from.
fn compatible<DB: sqlx::Database, V: Type<DB>>(infos: &[DB::TypeInfo]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for info in infos {
        let name = info.name().to_owned();
        if V::compatible(info) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn names(names: &[String]) -> Vec<&str> {
    names.iter().map(String::as_str).collect()
}

fn postgres_infos() -> Vec<<Postgres as sqlx::Database>::TypeInfo> {
    vec![
        <bool as Type<Postgres>>::type_info(),
        <i16 as Type<Postgres>>::type_info(),
        <i32 as Type<Postgres>>::type_info(),
        <i64 as Type<Postgres>>::type_info(),
        <f32 as Type<Postgres>>::type_info(),
        <f64 as Type<Postgres>>::type_info(),
        <String as Type<Postgres>>::type_info(),
        <Vec<u8> as Type<Postgres>>::type_info(),
        <NaiveDate as Type<Postgres>>::type_info(),
        <NaiveTime as Type<Postgres>>::type_info(),
        <NaiveDateTime as Type<Postgres>>::type_info(),
        <DateTime<Utc> as Type<Postgres>>::type_info(),
    ]
}

fn mysql_infos() -> Vec<<MySql as sqlx::Database>::TypeInfo> {
    vec![
        <bool as Type<MySql>>::type_info(),
        <i8 as Type<MySql>>::type_info(),
        <i16 as Type<MySql>>::type_info(),
        <i32 as Type<MySql>>::type_info(),
        <i64 as Type<MySql>>::type_info(),
        <u64 as Type<MySql>>::type_info(),
        <f32 as Type<MySql>>::type_info(),
        <f64 as Type<MySql>>::type_info(),
        <String as Type<MySql>>::type_info(),
        <Vec<u8> as Type<MySql>>::type_info(),
        <NaiveDate as Type<MySql>>::type_info(),
        <NaiveTime as Type<MySql>>::type_info(),
        <NaiveDateTime as Type<MySql>>::type_info(),
        <DateTime<Utc> as Type<MySql>>::type_info(),
    ]
}

fn sqlite_infos() -> Vec<<Sqlite as sqlx::Database>::TypeInfo> {
    vec![
        <bool as Type<Sqlite>>::type_info(),
        <i32 as Type<Sqlite>>::type_info(),
        <i64 as Type<Sqlite>>::type_info(),
        <f64 as Type<Sqlite>>::type_info(),
        <String as Type<Sqlite>>::type_info(),
        <Vec<u8> as Type<Sqlite>>::type_info(),
        <NaiveDate as Type<Sqlite>>::type_info(),
        <NaiveTime as Type<Sqlite>>::type_info(),
        <NaiveDateTime as Type<Sqlite>>::type_info(),
    ]
}

/// What a value's encoder gave: the bytes or text, or its error's message
/// (`panic` when it panicked).
fn encoded(row: Row, key: &str, result: Result<Result<String, String>, ()>) -> Row {
    match result {
        Ok(Ok(written)) => row.text(key, &written),
        Ok(Err(message)) => row.text("err", &message),
        Err(()) => row.text("err", "panic"),
    }
}

/// PostgreSQL: the type's name, then the bytes SQLx's `Encode` writes for
/// each value in the binary format (without the four-byte length a bind
/// message puts before them).
fn postgres<V>(g: &mut Golden, values: &[V])
where
    V: Value + Type<Postgres> + PgHasArrayType + for<'q> Encode<'q, Postgres>,
{
    let compatible = compatible::<Postgres, V>(&postgres_infos());
    g.push(
        POSTGRES,
        Row::of(V::TYPE)
            .text("type_info", V::type_info().name())
            .text("array_type_info", V::array_type_info().name())
            .list("compatible", &names(&compatible)),
    );
    for value in values {
        let result = guard(|| {
            let mut buffer = PgArgumentBuffer::default();
            Encode::<Postgres>::encode_by_ref(value, &mut buffer)
                .map(|_| hex(&buffer))
                .map_err(|error| error.to_string())
        });
        g.push(POSTGRES, encoded(value.row(), "bytes", result));
    }
}

/// MySQL: the type's name, then the bytes SQLx's `Encode` writes for each
/// value in the binary protocol (the length byte first).
fn mysql<V>(g: &mut Golden, values: &[V])
where
    V: Value + Type<MySql> + for<'q> Encode<'q, MySql>,
{
    let compatible = compatible::<MySql, V>(&mysql_infos());
    g.push(
        MYSQL,
        Row::of(V::TYPE)
            .text("type_info", V::type_info().name())
            .list("compatible", &names(&compatible)),
    );
    for value in values {
        let result = guard(|| {
            let mut buffer: Vec<u8> = Vec::new();
            Encode::<MySql>::encode_by_ref(value, &mut buffer)
                .map(|_| hex(&buffer))
                .map_err(|error| error.to_string())
        });
        g.push(MYSQL, encoded(value.row(), "bytes", result));
    }
}

/// The one text SQLx's `Encode` puts in SQLite's argument buffer. The
/// buffer shows its values only through `Debug`, so the text is taken from
/// there.
fn sqlite_text<V: for<'q> Encode<'q, Sqlite>>(value: &V) -> Result<String, String> {
    let mut buffer = SqliteArgumentsBuffer::default();
    let _is_null =
        Encode::<Sqlite>::encode_by_ref(value, &mut buffer).map_err(|error| error.to_string())?;
    let debug = format!("{buffer:?}");
    let text = debug
        .strip_prefix("SqliteArgumentsBuffer([Text(\"")
        .and_then(|rest| rest.strip_suffix("\")])"))
        .filter(|text| !text.contains(['"', '\\']))
        .unwrap_or_else(|| panic!("SQLx bound {debug} to SQLite, not one plain text"));
    Ok(text.to_owned())
}

/// SQLite: the type's name, then the text SQLx's `Encode` binds for each
/// value. `expected` is the expression SQLx's source uses; the two must
/// agree, which proves the text was taken from the buffer correctly.
fn sqlite<V>(g: &mut Golden, values: &[V], expected: impl Fn(&V) -> String) -> Result<(), String>
where
    V: Value + Type<Sqlite> + for<'q> Encode<'q, Sqlite>,
{
    let compatible = compatible::<Sqlite, V>(&sqlite_infos());
    g.push(
        SQLITE,
        Row::of(V::TYPE)
            .text("type_info", V::type_info().name())
            .list("compatible", &names(&compatible)),
    );
    for value in values {
        let result = guard(|| sqlite_text(value));
        if let (Ok(Ok(text)), Ok(expected)) = (&result, guard(|| expected(value)))
            && *text != expected
        {
            return Err(format!(
                "SQLx bound `{text}` to SQLite for {}, not `{expected}`",
                value.canon()
            ));
        }
        g.push(SQLITE, encoded(value.row(), "out", result));
    }
    Ok(())
}

fn databases(g: &mut Golden) -> Result<(), String> {
    let core = core_naive();
    let leaps = leap_naive();
    let sweep = sweep_naive();

    let instants = [utc(&core), utc(&leaps), utc(&sweep)].concat();
    let fixed = [core_fixed(), sweep_fixed()].concat();
    let zoned = core_zoned();
    let date_times = [core, leaps, sweep].concat();
    let dates = [core_dates(), month_ends()].concat();
    let times = [core_times(), leap_times()].concat();

    // PostgreSQL is sent an instant's UTC date and time whatever its
    // offset, so the month ends with an offset would repeat the UTC ones.
    postgres(g, &instants);
    postgres(g, &core_fixed());
    postgres(g, &zoned);
    postgres(g, &date_times);
    postgres(g, &dates);
    postgres(g, &times);

    mysql(g, &instants);
    mysql(g, &date_times);
    mysql(g, &dates);
    mysql(g, &times);

    sqlite(g, &instants, |at| {
        at.to_rfc3339_opts(SecondsFormat::AutoSi, false)
    })?;
    sqlite(g, &fixed, |at| {
        at.to_rfc3339_opts(SecondsFormat::AutoSi, false)
    })?;
    sqlite(g, &zoned, |at| {
        at.to_rfc3339_opts(SecondsFormat::AutoSi, false)
    })?;
    sqlite(g, &date_times, |at| at.format("%F %T%.f").to_string())?;
    sqlite(g, &dates, |day| day.format("%F").to_string())?;
    sqlite(g, &times, |time| time.format("%T%.f").to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Everything, and the README
// ---------------------------------------------------------------------------

fn generate() -> Result<Golden, String> {
    let mut g = Golden::default();
    let singles = single_patterns();
    let longer = longer_patterns();
    let patterns = [singles.clone(), longer.clone()].concat();
    if let Some(refused) = patterns.iter().find(|pattern| !pattern_ok(pattern)) {
        return Err(format!("chrono refuses the pattern `{refused}`"));
    }
    written_forms(&mut g);
    strftime_rows(&mut g, &singles, &longer);
    reading(&mut g, &patterns);
    // Every pattern used above is in `patterns.jsonl`.
    let used = g.patterns.clone();
    pattern_rows(&mut g, &used);
    local_times(&mut g);
    databases(&mut g)?;
    Ok(g)
}

/// The version of `package` in `Cargo.lock`.
fn locked(lock: &str, package: &str) -> Result<String, String> {
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == name
            && let Some(version) = lines
                .next()
                .and_then(|line| line.strip_prefix("version = \""))
                .and_then(|rest| rest.strip_suffix('"'))
        {
            return Ok(version.to_owned());
        }
    }
    Err(format!("Cargo.lock has no package {package}"))
}

/// `README.md`: what the files are, what made them and what each field
/// holds.
fn readme(root: &Path, golden: &Golden) -> Result<String, String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|e| format!("cannot read Cargo.lock: {e}"))?;
    let chrono = locked(&lock, "chrono")?;
    let sqlx = locked(&lock, "sqlx")?;
    for driver in ["sqlx-postgres", "sqlx-mysql", "sqlx-sqlite"] {
        if locked(&lock, driver)? != sqlx {
            return Err(format!("{driver} is not at sqlx's version {sqlx}"));
        }
    }
    let serde_json = locked(&lock, "serde_json")?;
    let tzdata = navree_time::tzdata_version();
    let count = |name: &str| golden.files.get(name).map_or(0, Vec::len);

    let mut text = README
        .replace("{chrono}", &chrono)
        .replace("{sqlx}", &sqlx)
        .replace("{serde_json}", &serde_json)
        .replace("{tzdata}", tzdata);
    for name in FILES {
        text = text.replace(&format!("{{lines:{name}}}"), &count(name).to_string());
    }
    if text.contains("{lines:") {
        return Err("the README names a file that is not written".to_owned());
    }
    Ok(text)
}

const README: &str = include_str!("time_golden.md");
