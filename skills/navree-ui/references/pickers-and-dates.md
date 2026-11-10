# Selects, dates and times, and the phone input

The controls that choose a value. Each block compiles on its own.

## Selects

Pick the control by how many choices there are and who holds them.

| Choices | Use |
|---|---|
| A few, fixed | `NativeSelect` (the browser's own) or `RadioGroup` |
| A short list the server holds | `Select`, in a form `widget = listbox`; the choices from `FormView::options` |
| One of a large or growing set | `Combobox` searched on the server |
| Several of a set | `MultiSelect`, in a form a `Many<T>` field |
| A palette for the whole app (Ctrl or Cmd and K) | `Command` |

```rust
use navree::ui::{Combobox, Command, CommandGroup, CommandItem, Field, MultiSelect, Select, icons};

pub fn stage_select() -> Field {
    Field::new("deal-stage", "Stage").control(
        Select::new("stage")
            .placeholder("Choose a stage")
            .option("lead", "Lead")
            .option("won", "Won")
            .option("lost", "Lost"),
    )
}

/// Asks `GET /people/options?q=text` as the person types. Only the chosen person is in the page.
pub fn owner_combobox() -> Field {
    Field::new("deal-owner", "Owner").control(
        Combobox::new("owner_id")
            .placeholder("Search people")
            .search("/people/options")
            .selected_choice(("7", "Harleen Kaur")),
    )
}

pub fn market_chips() -> Field {
    Field::new("deal-markets", "Markets").control(
        MultiSelect::new("markets")
            .placeholder("Add a market")
            .options([("in", "India"), ("ae", "UAE"), ("sg", "Singapore")])
            .selected_values(["in"])
            .max(3),
    )
}

pub fn palette() -> Command {
    Command::new("palette")
        .placeholder("Type a command or search...")
        .group(
            CommandGroup::new("Go to")
                .item(CommandItem::link("Contacts", "/contacts").icon(icons::USERS).shortcut("alt+c"))
                .item(CommandItem::link("New contact", "/contacts/new").icon(icons::PLUS)),
        )
}
```

The route behind a searched picker takes `OptionQuery` (the text, a limit of
at most 50 and an offset; it never fails) and answers `ChoiceList` built
by `OptionSource`, which searches the columns your code names on the
caller's own scoped query.
[`building-pages.md`](../../navree-design-to-ui/references/building-pages.md)
has the route and the check on save, in code that compiles. A searched
picker with JavaScript off shows its chosen option and cannot change it: use
a `Select` where the page must work without script.
A `Choice` can carry `.description(..)`, `.icon(..)`, `.avatar(src)` and
`.keywords(..)`; `.creatable()` and `.create_url(url)` let a person add an
option, and the route behind `create_url` must check that they may.

## Dates and times

Four components choose dates and times. A day is `NaiveDate`, never a point
in time: nothing in the toolkit passes a day through a time zone. "Today" is
a date the page says (`.today(date)`), so a page is the same wherever it is
read and a test can name its own today. The server decides: what is posted is
parsed and held to the same `DateRules`, `TimeRules` or `RangeRules` the
picker was drawn with.

| Choose | Use |
|---|---|
| A month grid, one day, several days or a range | `Calendar` |
| One day in a form | `DatePicker`, or `#[form(widget = date_picker)]` on a `NaiveDate` |
| A window of days, for a report | `DateRangePicker` in a `GET` form, read with `DateRangeQuery` |
| A time of day | `TimePicker`, or `#[form(widget = time_picker)]` on a `NaiveTime` |
| A day and a time together (a meeting, a delivery slot) | `DateTimePicker`: one value, `2026-11-12T14:30`, read back with `DateTimePicker::read`; with JavaScript off it is the browser's own date-and-time field |

```rust
use chrono::{NaiveDate, NaiveTime, Weekday};
use navree::ui::dates::{
    DatePicker, DatePreset, DateRange, DateRangePicker, DateRangeQuery, DateRules, TimePicker,
};
use navree::ui::{Button, Element, Field, Form, Stack, Validate, el};
use serde::Deserialize;

/// The pickers of a visit form. `today` comes from the caller: the toolkit never reads the clock.
pub fn visit_fields(today: NaiveDate) -> Stack {
    Stack::gap(4).children((
        Field::new("visit", "Visit on")
            .help("Type it as 11/30/2026, or pick it.")
            .control(DatePicker::new("visit").today(today).clearable().with_today()),
        Field::new("arrives", "Arrives at")
            .control(TimePicker::new("arrives").step_minutes(15)),
    ))
}

/// A report's window lives in the address: a `GET` form, so back, reload and a shared link show it.
pub fn report_filter(today: NaiveDate, window: Option<DateRange>) -> Element {
    let range = window.unwrap_or_else(|| DateRange::new(today - chrono::Duration::days(6), today));
    el("form")
        .attr("method", "get")
        .attr("action", "/reports/sales")
        .children((
            DateRangePicker::new("from", "to")
                .today(today)
                .title("Created")
                .presets([DatePreset::Today, DatePreset::Last7Days, DatePreset::ThisMonth])
                .value(range),
            Button::primary("Apply").submit(),
        ))
}

/// A booking form that holds its dates and time to rules the picker is also drawn with.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Booking {
    /// Start date
    #[validate(skip)]
    #[form(widget = date_picker, date_rules = weekdays_only, help = "A weekday.")]
    pub start: NaiveDate,
    /// Arrives at
    #[validate(skip)]
    #[form(widget = time_picker)]
    pub arrives: NaiveTime,
}

fn weekdays_only() -> DateRules {
    DateRules::new().disable_weekdays([Weekday::Sat, Weekday::Sun])
}

/// What the report route reads: `DateRangeQuery` keeps a bad or backwards window from failing the request.
pub fn window_of(query: &DateRangeQuery) -> Option<DateRange> {
    query.range()
}
```

- A window is whole days. `DateRange::utc_bounds(zone)` gives the two
  instants a query needs, `[start, end)`, right across days of 23 and 25
  hours. A time is on a wall: `dates::zoned::resolve(date, time, &zone)`
  says when a clock change makes it absent or doubled.
- `DateLocale::of("hi")` and 18 other locales set month and weekday names,
  the order of a typed date, the first weekday and the clock. India's week
  starts on Sunday. `FormView::dates(locale, today)` sets them for a form.
- A date and a time together are two fields. A single control for both is
  not built yet.
- Years 1 to 9999, Gregorian only. With JavaScript off a native `date` or
  `time` field stands in and posts the same value.

## The phone input

`PhoneInput` is a country list and the number, formatted as typed and read on
the server in the country chosen. In a form a `Phone` or `PhoneNumber` field
draws one, and `#[form(widget = phone)]` on a `String` holds the number in
E.164, refusing a number that is not valid or has an extension.
`FormView::phone_region(region)` sets the country phone fields start on.

```rust
use navree::ui::{Field, PhoneInput};

pub fn mobile_field() -> Field {
    Field::new("contact-mobile", "Mobile")
        .help("Type it as you would dial it, or paste it with its country code.")
        .control(PhoneInput::new("mobile").country("IN"))
}
```
