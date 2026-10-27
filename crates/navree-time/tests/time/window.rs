//! `SendWindow`: when people-facing messages may go out, on the recipient's clock.

use std::time::Duration;

use chrono::Weekday;
use navree_time::{FixedHolidays, Holidays, SendWindow, weekend_days};

use crate::support::{day, utc, zone};

fn office_hours() -> SendWindow {
    SendWindow::weekdays().between("09:00", "17:00")
}

#[test]
fn inside_the_window_a_message_goes_now() {
    let kolkata = zone("Asia/Kolkata");
    // Tuesday 27 October 2026, 11:00 in Kolkata.
    let now = utc("2026-10-27T05:30:00Z");
    assert!(office_hours().contains(now, &kolkata));
    assert_eq!(office_hours().next_slot(now, &kolkata), now);
    assert_eq!(office_hours().delay(now, &kolkata), Duration::ZERO);
}

#[test]
fn before_opening_it_waits_for_the_opening() {
    let kolkata = zone("Asia/Kolkata");
    // Tuesday 07:00 in Kolkata.
    let now = utc("2026-10-27T01:30:00Z");
    assert!(!office_hours().contains(now, &kolkata));
    assert_eq!(
        office_hours().next_slot(now, &kolkata),
        utc("2026-10-27T03:30:00Z")
    );
    assert_eq!(
        office_hours().delay(now, &kolkata),
        Duration::from_secs(2 * 3600)
    );
}

#[test]
fn after_closing_on_friday_it_waits_for_monday() {
    let kolkata = zone("Asia/Kolkata");
    // Friday 30 October 2026, 17:30 in Kolkata.
    let now = utc("2026-10-30T12:00:00Z");
    assert_eq!(
        office_hours().next_slot(now, &kolkata),
        utc("2026-11-02T03:30:00Z")
    );
    // Saturday too.
    assert_eq!(
        office_hours().next_slot(utc("2026-10-31T06:00:00Z"), &kolkata),
        utc("2026-11-02T03:30:00Z")
    );
}

#[test]
fn the_closing_minute_is_outside() {
    let utc_zone = zone("UTC");
    let at_close = utc("2026-10-27T17:00:00Z");
    assert!(!office_hours().contains(at_close, &utc_zone));
    assert!(office_hours().contains(utc("2026-10-27T16:59:59Z"), &utc_zone));
    assert!(office_hours().contains(utc("2026-10-27T09:00:00Z"), &utc_zone));
}

#[test]
fn the_same_window_opens_at_different_instants_per_zone() {
    let now = utc("2026-10-27T00:00:00Z"); // Tuesday
    let window = office_hours();
    assert_eq!(
        window.next_slot(now, &zone("Asia/Dubai")),
        utc("2026-10-27T05:00:00Z")
    );
    assert_eq!(
        window.next_slot(now, &zone("Europe/Paris")),
        utc("2026-10-27T08:00:00Z")
    );
    assert_eq!(
        window.next_slot(now, &zone("America/New_York")),
        utc("2026-10-27T13:00:00Z")
    );
}

#[test]
fn opening_hours_follow_daylight_saving_time() {
    let new_york = zone("America/New_York");
    // Friday 6 March 2026 (EST) and Monday 9 March 2026 (EDT).
    assert_eq!(
        office_hours().next_slot(utc("2026-03-06T00:00:00Z"), &new_york),
        utc("2026-03-06T14:00:00Z")
    );
    assert_eq!(
        office_hours().next_slot(utc("2026-03-06T23:00:00Z"), &new_york),
        utc("2026-03-09T13:00:00Z")
    );
}

#[test]
fn an_opening_time_inside_a_dst_gap_moves_forward() {
    let new_york = zone("America/New_York");
    let window = SendWindow::every_day().between("02:30", "04:00");
    // Sunday 8 March 2026: 02:30 does not exist; the window opens at 03:30 EDT.
    assert_eq!(
        window.next_slot(utc("2026-03-08T05:00:00Z"), &new_york),
        utc("2026-03-08T07:30:00Z")
    );
}

#[test]
fn holidays_are_skipped() {
    let london = zone("Europe/London");
    let holidays = Holidays::for_country("GB");
    let window = office_hours().except(holidays);
    // Thursday 24 December 2026, 18:00: Christmas (Friday), the weekend and
    // the Boxing Day substitute (Monday 28) are closed.
    assert_eq!(
        window.next_slot(utc("2026-12-24T18:00:00Z"), &london),
        utc("2026-12-29T09:00:00Z")
    );
    assert!(!window.contains(utc("2026-12-25T10:00:00Z"), &london));
}

#[test]
fn custom_calendars_close_their_days() {
    let calendar = FixedHolidays::new().add("FR", day("2026-10-27"), "Company day");
    let window = office_hours().except(Holidays::with_calendar("FR", calendar));
    let paris = zone("Europe/Paris");
    assert_eq!(
        window.next_slot(utc("2026-10-27T07:00:00Z"), &paris),
        utc("2026-10-28T08:00:00Z")
    );
}

#[test]
fn working_days_follow_the_country() {
    assert_eq!(weekend_days("US"), [Weekday::Sat, Weekday::Sun]);
    assert_eq!(weekend_days("SA"), [Weekday::Fri, Weekday::Sat]);
    assert_eq!(weekend_days("IN"), [Weekday::Sun]);
    assert_eq!(weekend_days("ae"), [Weekday::Sat, Weekday::Sun]);
    assert_eq!(weekend_days("XX"), [Weekday::Sat, Weekday::Sun]);

    let riyadh = zone("Asia/Riyadh");
    let window = SendWindow::working_days_in("SA").between("09:00", "17:00");
    assert_eq!(
        window.days(),
        [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Sun]
    );
    // Thursday 29 October 2026, 18:00 in Riyadh: next is Sunday 1 November.
    assert_eq!(
        window.next_slot(utc("2026-10-29T15:00:00Z"), &riyadh),
        utc("2026-11-01T06:00:00Z")
    );
}

#[test]
fn windows_are_built_from_days_and_times() {
    let saturdays = SendWindow::on(&[Weekday::Sat]).between("10:00", "12:00");
    assert_eq!(saturdays.days(), [Weekday::Sat]);
    assert_eq!(saturdays.start().to_string(), "10:00:00");
    assert_eq!(saturdays.end().map(|t| t.to_string()), Some("12:00:00".to_owned()));
    let all_day = SendWindow::always();
    assert_eq!(all_day.days().len(), 7);
    assert_eq!(all_day.end(), None, "open until midnight");
    let until_midnight = SendWindow::weekdays().between("18:00", "24:00");
    assert_eq!(until_midnight.end(), None);
    assert!(until_midnight.contains(utc("2026-10-27T23:59:00Z"), &zone("UTC")));
}

#[test]
fn malformed_or_backwards_times_are_refused() {
    for (start, end) in [("9am", "17:00"), ("09:00", "25:00"), ("17:00", "09:00"), ("09:00", "09:00"), ("24:00", "24:00")] {
        assert!(
            SendWindow::weekdays().try_between(start, end).is_err(),
            "{start}-{end}"
        );
    }
    let error = SendWindow::weekdays()
        .try_between("17:00", "09:00")
        .unwrap_err();
    assert!(error.to_string().contains("17:00"), "{error}");
}

#[test]
#[should_panic(expected = "send window")]
fn between_panics_on_a_malformed_literal() {
    let _ = SendWindow::weekdays().between("nine", "five");
}
