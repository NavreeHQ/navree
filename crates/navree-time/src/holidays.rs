//! [`Holidays`]: public holidays by country, extra closing days and custom
//! calendars ([`HolidayCalendar`]).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{NaiveDate, Weekday};
use serde::Serialize;

use crate::{DayRange, LocalDay};

/// One holiday: a day and its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, utoipa::ToSchema)]
pub struct Holiday {
    /// The day.
    pub date: LocalDay,
    /// Its name (`Christmas Day`).
    pub name: String,
}

impl Holiday {
    /// `name` on `date`.
    pub fn new(date: LocalDay, name: impl Into<String>) -> Self {
        Holiday {
            date,
            name: name.into(),
        }
    }
}

/// Where holidays come from: a driver that lists a country's holidays for
/// a year. [`BuiltInHolidays`] computes them for some countries;
/// [`FixedHolidays`] serves a list (the app's own data, or a test's).
pub trait HolidayCalendar: Send + Sync + 'static {
    /// The holidays of `country` (an upper-case ISO 3166-1 alpha-2 code)
    /// in `year`, in any order. A holiday observed on a day of the next or
    /// previous year (New Year's Day observed on 31 December) may be listed
    /// under the year it belongs to.
    fn holidays(&self, country: &str, year: i32) -> Vec<Holiday>;
}

/// Holidays computed from rules, for the countries in
/// [`BuiltInHolidays::countries`]: national public holidays only (no
/// regional ones), with the observed or substitute days each country
/// moves weekend holidays to. Holidays that follow a lunar calendar
/// (Diwali, Eid) and one-off holidays are not computed; add them with
/// [`Holidays::with_days`] or a calendar of your own.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltInHolidays;

impl BuiltInHolidays {
    /// The countries with rules: `AE`, `AU`, `CA`, `DE`, `FR`, `GB`
    /// (England and Wales), `IN`, `SA` and `US` (federal).
    pub fn countries() -> &'static [&'static str] {
        &["AE", "AU", "CA", "DE", "FR", "GB", "IN", "SA", "US"]
    }
}

impl HolidayCalendar for BuiltInHolidays {
    fn holidays(&self, country: &str, year: i32) -> Vec<Holiday> {
        let mut list = Rules::new(year);
        match country {
            "US" => list.united_states(),
            "GB" => list.england_and_wales(),
            "FR" => list.france(),
            "DE" => list.germany(),
            "IN" => list.india(),
            "AE" => list.united_arab_emirates(),
            "SA" => list.saudi_arabia(),
            "CA" => list.canada(),
            "AU" => list.australia(),
            _ => {}
        }
        list.holidays
    }
}

/// A calendar from a list of days: the app's own holiday data (loaded from
/// a table at boot) or a test's.
///
/// ```
/// use navree_time::{FixedHolidays, Holidays};
///
/// let calendar = FixedHolidays::new().add("NP", "2026-10-21".parse()?, "Dashain");
/// let nepal = Holidays::with_calendar("NP", calendar);
/// assert!(nepal.is_holiday("2026-10-21".parse()?));
/// # Ok::<(), navree_time::LocalDayError>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct FixedHolidays {
    days: Vec<(String, Holiday)>,
}

impl FixedHolidays {
    /// An empty calendar.
    pub fn new() -> Self {
        FixedHolidays::default()
    }

    /// Adds `name` on `day` for `country` (any case).
    pub fn add(mut self, country: &str, day: LocalDay, name: impl Into<String>) -> Self {
        self.days
            .push((country.trim().to_ascii_uppercase(), Holiday::new(day, name)));
        self
    }
}

impl HolidayCalendar for FixedHolidays {
    fn holidays(&self, country: &str, year: i32) -> Vec<Holiday> {
        self.days
            .iter()
            .filter(|(code, holiday)| code == country && holiday.date.year() == year)
            .map(|(_, holiday)| holiday.clone())
            .collect()
    }
}

/// The days a country (or an app) is closed: the public holidays of a
/// [`HolidayCalendar`] for one country, plus extra closing days.
///
/// Cheap to clone (it shares its calendar and a per-year cache).
/// `TimeModule` builds the app's default from `time.holidays`
/// ([`Zones::holidays`](crate::Zones::holidays)).
///
/// ```
/// use navree_time::Holidays;
///
/// let gb = Holidays::for_country("GB");
/// assert!(gb.is_holiday("2026-12-28".parse()?)); // Boxing Day, substitute
/// assert!(!gb.is_holiday("2026-12-24".parse()?));
/// # Ok::<(), navree_time::LocalDayError>(())
/// ```
#[derive(Clone)]
pub struct Holidays {
    country: Option<String>,
    calendar: Option<Arc<dyn HolidayCalendar>>,
    extra: Arc<BTreeSet<LocalDay>>,
    cache: Arc<Mutex<HashMap<i32, Arc<Vec<Holiday>>>>>,
}

/// The name of an extra closing day.
const CLOSED: &str = "Closed";

impl Holidays {
    /// No holidays.
    pub fn none() -> Self {
        Holidays {
            country: None,
            calendar: None,
            extra: Arc::default(),
            cache: Arc::default(),
        }
    }

    /// The public holidays of `country` (an ISO 3166-1 alpha-2 code, any
    /// case) from [`BuiltInHolidays`]; none for countries it has no rules
    /// for.
    pub fn for_country(country: &str) -> Self {
        Holidays::with_calendar(country, BuiltInHolidays)
    }

    /// The holidays of `country` from `calendar`.
    pub fn with_calendar(country: &str, calendar: impl HolidayCalendar) -> Self {
        Holidays::with_shared_calendar(country, Arc::new(calendar))
    }

    /// The holidays of `country` from a shared calendar.
    pub fn with_shared_calendar(country: &str, calendar: Arc<dyn HolidayCalendar>) -> Self {
        Holidays {
            country: Some(country.trim().to_ascii_uppercase()),
            calendar: Some(calendar),
            extra: Arc::default(),
            cache: Arc::default(),
        }
    }

    /// Adds extra closing days (named `Closed`).
    pub fn with_days(self, days: impl IntoIterator<Item = LocalDay>) -> Self {
        let mut extra = (*self.extra).clone();
        extra.extend(days);
        Holidays {
            extra: Arc::new(extra),
            cache: Arc::default(),
            ..self
        }
    }

    /// The country, upper-case, if any.
    pub fn country(&self) -> Option<&str> {
        self.country.as_deref()
    }

    /// Every holiday in `year`, by date.
    pub fn in_year(&self, year: i32) -> Vec<Holiday> {
        self.year(year).as_ref().clone()
    }

    /// The holiday on `day`, if it is one.
    pub fn holiday(&self, day: LocalDay) -> Option<Holiday> {
        self.year(day.year())
            .iter()
            .find(|holiday| holiday.date == day)
            .cloned()
    }

    /// Whether `day` is a holiday.
    pub fn is_holiday(&self, day: LocalDay) -> bool {
        self.year(day.year())
            .iter()
            .any(|holiday| holiday.date == day)
    }

    /// The holidays within `range`, by date.
    pub fn between(&self, range: &DayRange) -> Vec<Holiday> {
        (range.from.year()..=range.to.year())
            .flat_map(|year| self.in_year(year))
            .filter(|holiday| range.contains(holiday.date))
            .collect()
    }

    /// The holidays dated in `year` (cached).
    fn year(&self, year: i32) -> Arc<Vec<Holiday>> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .entry(year)
            .or_insert_with(|| Arc::new(self.compute(year)))
            .clone()
    }

    fn compute(&self, year: i32) -> Vec<Holiday> {
        let mut holidays: Vec<Holiday> = Vec::new();
        if let (Some(country), Some(calendar)) = (&self.country, &self.calendar) {
            // Observed days can move across a new year: ask for the
            // neighbouring years too and keep what is dated in `year`.
            for rule_year in [year - 1, year, year + 1] {
                holidays.extend(
                    calendar
                        .holidays(country, rule_year)
                        .into_iter()
                        .filter(|holiday| holiday.date.year() == year),
                );
            }
        }
        for day in self.extra.iter().filter(|day| day.year() == year) {
            if !holidays.iter().any(|holiday| holiday.date == *day) {
                holidays.push(Holiday::new(*day, CLOSED));
            }
        }
        holidays.sort();
        holidays.dedup_by(|a, b| a.date == b.date && a.name == b.name);
        holidays
    }
}

impl Default for Holidays {
    fn default() -> Self {
        Holidays::none()
    }
}

impl fmt::Debug for Holidays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Holidays")
            .field("country", &self.country)
            .field("extra", &self.extra)
            .finish_non_exhaustive()
    }
}

/// Easter Sunday of `year` in the Gregorian calendar (the anonymous
/// Gregorian computus).
pub fn easter_sunday(year: i32) -> LocalDay {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    LocalDay::new(year, month as u32, day as u32).expect("the computus gives a valid date")
}

/// Builds one year's holidays for a country.
struct Rules {
    year: i32,
    holidays: Vec<Holiday>,
}

impl Rules {
    fn new(year: i32) -> Self {
        Rules {
            year,
            holidays: Vec::new(),
        }
    }

    fn date(&self, month: u32, day: u32) -> LocalDay {
        LocalDay::new(self.year, month, day).expect("a fixed holiday date exists every year")
    }

    fn add(&mut self, day: LocalDay, name: &str) {
        self.holidays.push(Holiday::new(day, name));
    }

    fn fixed(&mut self, month: u32, day: u32, name: &str) {
        let date = self.date(month, day);
        self.add(date, name);
    }

    fn easter(&self, offset: i64) -> LocalDay {
        easter_sunday(self.year).add_days(offset)
    }

    /// The `n`th `weekday` of `month` (1-based).
    fn nth(&self, month: u32, weekday: Weekday, n: u32) -> LocalDay {
        let date = NaiveDate::from_weekday_of_month_opt(self.year, month, weekday, n as u8)
            .expect("the first four weekdays of a month exist");
        LocalDay::from(date)
    }

    /// The last `weekday` of `month`.
    fn last(&self, month: u32, weekday: Weekday) -> LocalDay {
        let mut day = self.date(month, 1).last_of_month();
        while day.weekday() != weekday {
            day = day.pred();
        }
        day
    }

    /// A fixed date moved to Friday when on a Saturday and to Monday when
    /// on a Sunday (United States federal rule).
    fn observed(&mut self, month: u32, day: u32, name: &str) {
        let date = self.date(month, day);
        let observed = match date.weekday() {
            Weekday::Sat => date.pred(),
            Weekday::Sun => date.succ(),
            _ => date,
        };
        if observed == date {
            self.add(date, name);
        } else {
            self.add(observed, &format!("{name} (observed)"));
        }
    }

    /// A fixed date moved to the next Monday when it falls on a weekend.
    fn next_monday(&mut self, month: u32, day: u32, name: &str) {
        let date = self.date(month, day);
        match date.weekday() {
            Weekday::Sat => self.add(date.add_days(2), &format!("{name} (substitute day)")),
            Weekday::Sun => self.add(date.succ(), &format!("{name} (substitute day)")),
            _ => self.add(date, name),
        }
    }

    /// Christmas Day and Boxing Day with substitute weekdays (England and
    /// Wales, Australia).
    fn christmas_and_boxing_day(&mut self) {
        let christmas = self.date(12, 25);
        let substitute = |name: &str| format!("{name} (substitute day)");
        match christmas.weekday() {
            Weekday::Fri => {
                self.add(christmas, "Christmas Day");
                self.add(christmas.add_days(3), &substitute("Boxing Day"));
            }
            Weekday::Sat => {
                self.add(christmas.add_days(2), &substitute("Christmas Day"));
                self.add(christmas.add_days(3), &substitute("Boxing Day"));
            }
            Weekday::Sun => {
                self.add(christmas.succ(), "Boxing Day");
                self.add(christmas.add_days(2), &substitute("Christmas Day"));
            }
            _ => {
                self.add(christmas, "Christmas Day");
                self.add(christmas.succ(), "Boxing Day");
            }
        }
    }

    fn united_states(&mut self) {
        self.observed(1, 1, "New Year's Day");
        let mlk = self.nth(1, Weekday::Mon, 3);
        self.add(mlk, "Martin Luther King Jr. Day");
        let presidents = self.nth(2, Weekday::Mon, 3);
        self.add(presidents, "Presidents' Day");
        let memorial = self.last(5, Weekday::Mon);
        self.add(memorial, "Memorial Day");
        if self.year >= 2021 {
            self.observed(6, 19, "Juneteenth");
        }
        self.observed(7, 4, "Independence Day");
        let labor = self.nth(9, Weekday::Mon, 1);
        self.add(labor, "Labor Day");
        let columbus = self.nth(10, Weekday::Mon, 2);
        self.add(columbus, "Columbus Day");
        self.observed(11, 11, "Veterans Day");
        let thanksgiving = self.nth(11, Weekday::Thu, 4);
        self.add(thanksgiving, "Thanksgiving Day");
        self.observed(12, 25, "Christmas Day");
    }

    fn england_and_wales(&mut self) {
        self.next_monday(1, 1, "New Year's Day");
        let good_friday = self.easter(-2);
        self.add(good_friday, "Good Friday");
        let easter_monday = self.easter(1);
        self.add(easter_monday, "Easter Monday");
        let early_may = self.nth(5, Weekday::Mon, 1);
        self.add(early_may, "Early May bank holiday");
        let spring = self.last(5, Weekday::Mon);
        self.add(spring, "Spring bank holiday");
        let summer = self.last(8, Weekday::Mon);
        self.add(summer, "Summer bank holiday");
        self.christmas_and_boxing_day();
    }

    fn france(&mut self) {
        self.fixed(1, 1, "New Year's Day");
        let easter_monday = self.easter(1);
        self.add(easter_monday, "Easter Monday");
        self.fixed(5, 1, "Labour Day");
        self.fixed(5, 8, "Victory in Europe Day");
        let ascension = self.easter(39);
        self.add(ascension, "Ascension Day");
        let whit_monday = self.easter(50);
        self.add(whit_monday, "Whit Monday");
        self.fixed(7, 14, "Bastille Day");
        self.fixed(8, 15, "Assumption Day");
        self.fixed(11, 1, "All Saints' Day");
        self.fixed(11, 11, "Armistice Day");
        self.fixed(12, 25, "Christmas Day");
    }

    fn germany(&mut self) {
        self.fixed(1, 1, "New Year's Day");
        let good_friday = self.easter(-2);
        self.add(good_friday, "Good Friday");
        let easter_monday = self.easter(1);
        self.add(easter_monday, "Easter Monday");
        self.fixed(5, 1, "Labour Day");
        let ascension = self.easter(39);
        self.add(ascension, "Ascension Day");
        let whit_monday = self.easter(50);
        self.add(whit_monday, "Whit Monday");
        self.fixed(10, 3, "German Unity Day");
        self.fixed(12, 25, "Christmas Day");
        self.fixed(12, 26, "Second Day of Christmas");
    }

    fn india(&mut self) {
        self.fixed(1, 26, "Republic Day");
        self.fixed(8, 15, "Independence Day");
        self.fixed(10, 2, "Gandhi Jayanti");
    }

    fn united_arab_emirates(&mut self) {
        self.fixed(1, 1, "New Year's Day");
        self.fixed(12, 2, "National Day");
        self.fixed(12, 3, "National Day holiday");
    }

    fn saudi_arabia(&mut self) {
        if self.year >= 2022 {
            self.fixed(2, 22, "Founding Day");
        }
        self.fixed(9, 23, "National Day");
    }

    fn canada(&mut self) {
        self.fixed(1, 1, "New Year's Day");
        let good_friday = self.easter(-2);
        self.add(good_friday, "Good Friday");
        // The Monday before 25 May.
        let mut victoria = self.date(5, 24);
        while victoria.weekday() != Weekday::Mon {
            victoria = victoria.pred();
        }
        self.add(victoria, "Victoria Day");
        let canada_day = self.date(7, 1);
        if canada_day.weekday() == Weekday::Sun {
            self.add(canada_day.succ(), "Canada Day (observed)");
        } else {
            self.add(canada_day, "Canada Day");
        }
        let labour = self.nth(9, Weekday::Mon, 1);
        self.add(labour, "Labour Day");
        if self.year >= 2021 {
            self.fixed(9, 30, "National Day for Truth and Reconciliation");
        }
        let thanksgiving = self.nth(10, Weekday::Mon, 2);
        self.add(thanksgiving, "Thanksgiving");
        self.fixed(11, 11, "Remembrance Day");
        self.fixed(12, 25, "Christmas Day");
        self.fixed(12, 26, "Boxing Day");
    }

    fn australia(&mut self) {
        self.next_monday(1, 1, "New Year's Day");
        self.next_monday(1, 26, "Australia Day");
        let good_friday = self.easter(-2);
        self.add(good_friday, "Good Friday");
        let easter_monday = self.easter(1);
        self.add(easter_monday, "Easter Monday");
        self.fixed(4, 25, "Anzac Day");
        self.christmas_and_boxing_day();
    }
}
