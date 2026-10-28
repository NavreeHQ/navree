//! Records croner's next 10 runs for a fixed set of expressions, zones and
//! start instants: the evidence our own cron parser is compared with
//! (`tests/cron/differential.rs`, data in `tests/data/croner_next_runs.txt`).
//!
//! This generator needs croner, which is removed from the crate once the
//! table is committed; it stays in the history only.
//!
//! ```text
//! cargo run -p navree-schedule --example croner_table > crates/navree-schedule/tests/data/croner_next_runs.txt
//! ```

use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::parser::{CronParser, Seconds};

/// Every documented form, plus the forms croner accepted in the current
/// configuration (five fields; seconds not allowed).
const DOCUMENTED: &[&str] = &[
    "* * * * *",
    "*/5 * * * *",
    "*/15 * * * *",
    "0 * * * *",
    "17 * * * *",
    "0 0 * * *",
    "0 8 * * *",
    "30 3 * * SUN",
    "0 0 * * 0",
    "0 0 * * 7",
    "0 6 * * MON-FRI",
    "*/10 9-17 * * MON-FRI",
    "0 9 * * mon-fri",
    "0 9 * * Mon,Wed,Fri",
    "0,30 * * * *",
    "5,20,35,50 * * * *",
    "0-10 * * * *",
    "5-59/10 * * * *",
    "10-40/7 * * * *",
    "*/7 * * * *",
    "*/20 */2 * * *",
    "0 */6 * * *",
    "0 1-23/2 * * *",
    "0 0 1 * *",
    "0 0 1,15 * *",
    "0 0 */10 * *",
    "0 0 1-7 * *",
    "0 0 31 * *",
    "0 0 30 * *",
    "0 0 29 2 *",
    "0 0 1 1 *",
    "0 0 1 JAN *",
    "0 0 1 jan,jul *",
    "0 0 1 FEB-APR *",
    "0 0 1 */3 *",
    "0 0 1 1-6 *",
    "0 12 15 6 *",
    "0 12 * 12 *",
    "0 0 * * 1",
    "0 0 * * 6,0",
    "0 0 * * 1-5",
    "0 0 * * 5-7",
    "0 0 * * FRI-SUN",
    "0 0 * * SUN-TUE",
    "0 0 * * */2",
    "0 0 * * 1-5/2",
    "0 0 ? * MON",
    "0 0 * * ?",
    "0 0 ? * ?",
    "0 0 L * *",
    "0 0 LW * *",
    "0 0 15W * *",
    "0 0 1W * *",
    "0 0 31W * *",
    "0 0 1,15W,L * *",
    "0 0 L FEB *",
    "0 0 * * 5L",
    "0 0 * * FRIL",
    "0 0 * * 0L",
    "0 0 * * 5#2",
    "0 0 * * FRI#2",
    "0 0 * * 1#1",
    "0 0 * * 3#5",
    "0 0 * * 5#L",
    "0 0 * * 7#2",
    "0 0 * * 1#1,3#3",
    "0 0 1-7 * +MON",
    "0 0 13 * +FRI",
    "0 0 1 1 +*",
    "0 0 29 2 +MON",
    "0 0 1,15 * MON",
    "0 0 1 * MON",
    "0 0 15 * 5L",
    "0 0 L * MON",
    "0 0 * 2 *",
    "0 0 29 2 MON",
    "0 0 30 2 *",
    "0 0 31 4,6,9,11 *",
    "0 0 30 FEB *",
    "30 2 * * *",
    "30 1 * * *",
    "0 2 * * *",
    "0 3 * * *",
    "0 1 * * *",
    "59 1 * * *",
    "*/30 * * * *",
    "0 0,12 * * *",
    "0 0 * * 0,6",
    "15 14 1 * *",
    "0 22 * * 1-5",
    "45 23 L * *",
    "0 5 * * 1#1",
    "@hourly",
    "@daily",
    "@weekly",
    "@monthly",
    "@yearly",
    "@annually",
    "@DAILY",
    "  0   9  *  *  1-5  ",
    "00 06 01 01 00",
    "0\t0\t*\t*\t*",
    "0 0 * * mon",
    "0 0 * JAN-MAR *",
    "0 0 1 */2 *",
    "0 0 */2 */2 *",
    "0 0 1-31/10 * *",
    "*/59 * * * *",
    "*/100 * * * *",
    "0 */25 * * *",
];

/// Expressions that run in the small hours, where clocks change.
const DST_HOURS: &[&str] = &[
    "* * * * *",
    "*/15 * * * *",
    "*/30 * * * *",
    "0 * * * *",
    "30 * * * *",
    "0 1 * * *",
    "30 1 * * *",
    "59 1 * * *",
    "0 2 * * *",
    "30 2 * * *",
    "59 2 * * *",
    "0 3 * * *",
    "30 3 * * *",
    "0 0 * * *",
    "0 4 * * *",
    "*/15 1-3 * * *",
    "*/10 0-4 * * *",
    "0,30 1,2,3 * * *",
    "15,45 1-2 * * *",
    "0 1-3 * * *",
    "30 0-4 * * *",
    "*/20 2 * * *",
    "*/5 1 * * *",
    "*/5 2 * * *",
    "0 */2 * * *",
    "0 1-23/2 * * *",
    "0 2 * * SUN",
    "30 2 * * 0",
    "30 1 * * SUN",
    "30 2 8 3 *",
    "30 2 1 11 *",
    "30 1 1 11 *",
    "30 1 25 10 *",
    "30 1 29 3 *",
    "0 2 L * *",
    "0 2 1 * *",
    "0 1 * * 1-5",
    "0 2 * * 1-5",
    "30 2 * * MON-FRI",
    "*/45 * * * *",
    "10 1 * * *",
    "10 2 * * *",
    "45 1 * * *",
    "45 2 * * *",
    "0 1,2 * * *",
    "0 2,3 * * *",
    "59 * * * *",
    "1 * * * *",
    "*/3 1-2 * * *",
    "*/7 2 * * *",
    "0 0-3 * * *",
    "0 */3 * * *",
    "15 2 * * SUN",
    "0 1 * * SUN#1",
    "30 2 * * 0#2",
    "30 1 * * 0L",
    "0 3 8 3 *",
    "0 2 29 3 *",
    "0 1 25 10 *",
    "0 1 1 11 *",
    "*/30 1-2 8 3 *",
    "*/30 0-3 1 11 *",
    "*/30 0-3 25 10 *",
    "*/30 0-3 29 3 *",
];

/// (zone, start) pairs for every expression: not near a clock change.
const GENERAL_STARTS: &[&str] = &["2026-01-15T10:07:30Z"];
const GENERAL_ZONES: &[&str] = &["UTC", "Asia/Kolkata", "America/New_York", "Europe/London"];
/// A UTC start just before a leap day.
const LEAP_START: &str = "2028-02-28T23:58:00Z";

/// (zone, start) pairs for the small-hours expressions: 90 minutes before
/// each change, and inside the repeated hour.
const DST_STARTS: &[(&str, &str)] = &[
    // New York: 02:00 EST -> 03:00 EDT at 2026-03-08T07:00Z.
    ("America/New_York", "2026-03-08T05:30:00Z"),
    // New York: 02:00 EDT -> 01:00 EST at 2026-11-01T06:00Z.
    ("America/New_York", "2026-11-01T04:30:00Z"),
    ("America/New_York", "2026-11-01T06:20:30Z"),
    // London: 01:00 GMT -> 02:00 BST at 2026-03-29T01:00Z.
    ("Europe/London", "2026-03-28T23:30:00Z"),
    // London: 02:00 BST -> 01:00 GMT at 2026-10-25T01:00Z.
    ("Europe/London", "2026-10-24T23:30:00Z"),
    ("Europe/London", "2026-10-25T01:20:30Z"),
];

/// Strings whose acceptance by croner is recorded (`parse | accepted/rejected | text`).
const PROBES: &[&str] = &[
    "61 * * * *",
    "60 * * * *",
    "* 24 * * *",
    "* * 0 * *",
    "* * 32 * *",
    "* * * 0 *",
    "* * * 13 *",
    "* * * * 8",
    "* * * * 9",
    "* * * FOO *",
    "* * * * FUN",
    "* * * * MONDAY",
    "* * * JANUARY *",
    "MON * * * *",
    "* MON * * *",
    "* * MON * *",
    "* * * MON *",
    "JAN * * * *",
    "* * JAN * *",
    "*/0 * * * *",
    "5/15 * * * *",
    "/15 * * * *",
    "0/15 * * * *",
    "1,,2 * * * *",
    "1, * * * *",
    ",1 * * * *",
    ", * * * *",
    "10-5 * * * *",
    "1-2-3 * * * *",
    "-5 * * * *",
    "5- * * * *",
    "* * 15L * *",
    "* * W * *",
    "* * 0W * *",
    "* * 32W * *",
    "* * L-3 * *",
    "* * 15W/2 * *",
    "* * LW,L * *",
    "* * * * L",
    "* * * * 5#6",
    "* * * * 5#0",
    "* * * * 5#",
    "* * * * #2",
    "* * * * 1-5#2",
    "* * * * 9L",
    "* * * * 1+",
    "* * * * +",
    "* * * * ++1",
    "* * * * 5L,5#2",
    "*/-1 * * * *",
    "1.5 * * * *",
    "*,1 * * * *",
    "*/5,1 * * * *",
    "0 0 * * *,1",
    "0 0 ?/5 * *",
    "0 0 ?? * *",
    "? * * * *",
    "* ? * * *",
    "0 0 0 * * *",
    "0 0 0 * * * 2026",
    "* * * *",
    "*",
    "",
    "   ",
    "@daily now",
    "@reboot",
    "@midnight",
    "@every_hour",
    "@",
    "@@daily",
    "0 0 @daily * *",
    "* * * * * *",
    "\u{e9} * * * *",
    "*/99999 * * * *",
    "0099 * * * *",
    "+5 * * * *",
    "*/+5 * * * *",
    "5-+7 * * * *",
    "0 0 1 * 5-7",
    "0 0 1 * 7-5",
    "0 0 1 * 0-7",
    "0 0 1 * SUN-SUN",
    "0 0 1 * SAT-SUN",
    "0 0 1 * FRI-MON",
    "0 0 1 * 7#2",
    "0 0 1 * 7L",
    "0 0 L,15 * *",
    "0 0 L-1 * *",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A numeric field in `min..=max`.
    fn numeric(&mut self, min: u64, max: u64) -> String {
        let span = max - min + 1;
        let value = |rng: &mut Self| min + rng.below(span);
        match self.below(7) {
            0 => "*".to_owned(),
            1 => {
                let steps = [1, 2, 3, 4, 5, 6, 7, 10, 12, 15, 20, 30];
                format!("*/{}", steps[self.below(steps.len() as u64) as usize])
            }
            2 => value(self).to_string(),
            3 => {
                let (a, b) = (value(self), value(self));
                format!("{}-{}", a.min(b), a.max(b))
            }
            4 => {
                let (a, b) = (value(self), value(self));
                format!("{}-{}/{}", a.min(b), a.max(b), 1 + self.below(9))
            }
            5 => {
                let mut values: Vec<u64> = (0..2 + self.below(3)).map(|_| value(self)).collect();
                values.sort_unstable();
                values.dedup();
                values
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
            _ => value(self).to_string(),
        }
    }

    fn day_of_month(&mut self) -> String {
        match self.below(9) {
            0 | 1 => "*".to_owned(),
            2 => "L".to_owned(),
            3 => "LW".to_owned(),
            4 => format!("{}W", 1 + self.below(31)),
            5 => "?".to_owned(),
            _ => self.numeric(1, 31),
        }
    }

    fn month(&mut self) -> String {
        const NAMES: [&str; 12] = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        match self.below(6) {
            0 => {
                let (a, b) = (self.below(12) as usize, self.below(12) as usize);
                format!("{}-{}", NAMES[a.min(b)], NAMES[a.max(b)])
            }
            1 => {
                let (a, b) = (self.below(12) as usize, self.below(12) as usize);
                format!("{},{}", NAMES[a.min(b)], NAMES[a.max(b).max(a.min(b) + 1) % 12])
            }
            _ => self.numeric(1, 12),
        }
    }

    fn day_of_week(&mut self) -> String {
        const NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        let body = match self.below(11) {
            0 | 1 | 2 => "*".to_owned(),
            3 => format!("{}L", self.below(7)),
            4 => format!("{}#{}", self.below(7), 1 + self.below(5)),
            5 => NAMES[self.below(7) as usize].to_owned(),
            6 => {
                let (a, b) = (self.below(7) as usize, self.below(7) as usize);
                format!("{}-{}", NAMES[a.min(b)], NAMES[a.max(b)])
            }
            7 => "?".to_owned(),
            _ => self.numeric(0, 6),
        };
        if body != "*" && self.below(8) == 0 {
            format!("+{body}")
        } else {
            body
        }
    }

    fn expression(&mut self) -> String {
        format!(
            "{} {} {} {} {}",
            self.numeric(0, 59),
            self.numeric(0, 23),
            self.day_of_month(),
            self.month(),
            self.day_of_week()
        )
    }
}

fn main() {
    let parser = CronParser::builder().seconds(Seconds::Disallowed).build();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut expressions: Vec<String> = DOCUMENTED.iter().map(|s| (*s).to_owned()).collect();
    let mut random = Vec::new();
    while random.len() < 200 {
        let expr = rng.expression();
        if parser.parse(&expr).is_ok() && !expressions.contains(&expr) && !random.contains(&expr) {
            random.push(expr);
        }
    }
    expressions.extend(random);

    println!("# Next 10 runs recorded from croner 4.0.2 (parser: five fields, seconds disallowed).");
    println!("# Generated by crates/navree-schedule/examples/croner_table.rs; see tests/cron/differential.rs.");
    println!("# Columns: zone | start (unix seconds) | expression | up to 10 next runs (unix minutes), `-` for none");
    println!("# Rows croner rejects are noted as `# rejected by croner: <expression>`.");
    for probe in PROBES {
        let verdict = if parser.parse(probe).is_ok() {
            "accepted"
        } else {
            "rejected"
        };
        println!("parse | {verdict} | {probe}");
    }
    let mut rows = 0;
    let mut record = |zone: &str, start: &str, expr: &str| {
        let Ok(cron) = parser.parse(expr) else {
            println!("# rejected by croner: {expr}");
            return;
        };
        let tz = Tz::from_str(zone).expect("zone");
        let start_utc = DateTime::parse_from_rfc3339(start)
            .expect("start")
            .with_timezone(&Utc);
        let runs: Vec<String> = cron
            .iter_after(start_utc.with_timezone(&tz))
            .take(10)
            .map(|run| {
                let secs = run.with_timezone(&Utc).timestamp();
                assert_eq!(secs % 60, 0, "{expr} {zone} {start}");
                (secs / 60).to_string()
            })
            .collect();
        let runs = if runs.is_empty() {
            "-".to_owned()
        } else {
            runs.join(",")
        };
        println!("{zone} | {} | {expr} | {runs}", start_utc.timestamp());
        rows += 1;
    };
    for expr in &expressions {
        for zone in GENERAL_ZONES {
            for start in GENERAL_STARTS {
                record(zone, start, expr);
            }
        }
        record("UTC", LEAP_START, expr);
    }
    for expr in DST_HOURS {
        for (zone, start) in DST_STARTS {
            record(zone, start, expr);
        }
    }
    eprintln!("{rows} rows, {} expressions", expressions.len());
}
