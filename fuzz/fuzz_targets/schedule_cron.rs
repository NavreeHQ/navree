//! `navree_schedule::CronExpression`: parse, then `next_after` and `is_due`
//! on fixed and daylight-saving zones.
//!
//! Input: a 15-byte header (zone, instant, offset), then the expression as
//! text.
//!
//! Invariants: an expression prints as written and parses again to an equal
//! value; the next run is strictly after the instant it was asked from, is
//! due by `is_due`, and no minute in between is due for a wildcard
//! expression on a fixed offset.
#![no_main]

use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use chrono_tz::Tz;
use libfuzzer_sys::fuzz_target;
use navree_schedule::CronExpression;

const ZONES: [Tz; 6] = [
    Tz::America__New_York,
    Tz::Europe__London,
    Tz::Australia__Lord_Howe,
    Tz::Asia__Kolkata,
    Tz::America__St_Johns,
    Tz::Pacific__Apia,
];

fuzz_target!(|data: &[u8]| {
    let Some((header, text)) = data.split_first_chunk::<15>() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let zone = ZONES[usize::from(header[0]) % ZONES.len()];
    let near = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
    let far = i64::from_le_bytes([
        header[5], header[6], header[7], header[8], header[9], header[10], header[11], header[12],
    ]);
    let offset_minutes = i16::from_le_bytes([header[13], header[14]]);

    let Ok(cron) = CronExpression::parse(text) else {
        return;
    };
    let again = CronExpression::parse(&cron.to_string()).expect("a printed expression parses");
    assert_eq!(again, cron, "parse, print and parse differ");
    let _ = cron.follows_wall_clock();

    // Instants from the whole range and from the neighbourhood of "now",
    // where the daylight saving changes of the zones above are.
    let after = if header[0] % 2 == 0 {
        DateTime::from_timestamp(far, 0)
    } else {
        DateTime::from_timestamp(1_700_000_000 + i64::from(near % 20_000_000) * 60, 0)
    };
    let Some(after) = after else { return };

    if let Some(next) = cron.next_after(after, &zone) {
        assert!(next > after, "{cron} on {zone}: {next} is not after {after}");
        assert!(cron.is_due(next, &zone), "{cron} on {zone}: {next} is not due");
    }
    if let Some(next) = cron.next_after(after, &Utc) {
        assert!(next > after);
        assert!(cron.is_due(next, &Utc));
        if cron.follows_wall_clock() {
            // Nothing is due between `after` and the next run (checked for
            // a few minutes: the search is exhaustive on a fixed offset).
            let mut minute =
                after.checked_add_signed(TimeDelta::seconds(60 - after.timestamp().rem_euclid(60)));
            for _ in 0..16 {
                match minute {
                    Some(m) if m < next => {
                        assert!(!cron.is_due(m, &Utc), "{cron}: {m} is due before {next}");
                        minute = m.checked_add_signed(TimeDelta::minutes(1));
                    }
                    _ => break,
                }
            }
        }
    }
    let offset = i32::from(offset_minutes) * 60;
    if let Some(fixed) = FixedOffset::east_opt(offset.clamp(-86_399, 86_399)) {
        if let Some(next) = cron.next_after(after, &fixed) {
            assert!(next > after);
            assert!(cron.is_due(next, &fixed));
        }
    }
});
