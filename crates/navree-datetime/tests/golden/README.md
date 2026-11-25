# Golden files: how Navree writes and reads dates and times

These files record what Navree's dates and times look like today, value by
value: their `Display` and `Debug` text, RFC 3339, RFC 2822, JSON, every
`strftime` specifier, what is sent to PostgreSQL, MySQL and SQLite, and
which texts are read as which values. `navree-datetime` is tested against
them. What a file says is written is what the library writes; what a file
says is accepted or refused is what the library accepts or refuses. While
that holds, nothing Navree has stored, sent or shown changes.

## What they were recorded from

| | Version |
|---|---|
| chrono, with its `serde` feature | 0.4.45 |
| SQLx (`sqlx`, `sqlx-postgres`, `sqlx-mysql`, `sqlx-sqlite`), with its `chrono` feature | 0.9.0 |
| serde_json | 1.0.154 |
| The IANA time zone database in navree-time's tables | 2025b |

The program is `xtask/src/time_golden.rs`, and this file is its
`time_golden.md`:

```
cargo xtask time-golden          # writes every file and this README
cargo xtask time-golden --check  # writes nothing, and fails when a file differs
```

It reads no clock, no locale and no environment variable, and ends lines
with `\n`, so every run on every machine writes the same bytes. No file here
is edited by hand. The program needs chrono in the workspace; when chrono
is gone, the files stay as they were recorded.

## A line

Each line is one JSON object. Its keys are always in the same order. The
text is ASCII: any other character is a `\u` escape, so a no-break space or
a minus sign can be seen.

| Key | Holds |
|---|---|
| `type` | The type the line is about (the table below). |
| `value` | Where a value is written: the value, as canonical text. In `parse_accept.jsonl` and `parse_refuse.jsonl`: the text that was read. |
| numbers | The value again as numbers (the table below), so a test builds it without parsing any text. |
| `leap` | `true` when the value holds a leap second (below); absent otherwise. |
| `out` | What was written. In `parse_accept.jsonl`, the canonical text of the value that was read (its numbers follow); in `parse_refuse.jsonl`, why the text was refused. |
| `err` | Stands in for `out` when nothing was written: `format` when the formatter returned an error (where `to_string()` panics), `panic` when the call panicked, or, in a database file, the encoder's own message. |

| `type` | Canonical text | Numbers | In `navree-datetime` |
|---|---|---|---|
| `DateTime<Utc>` | `2026-01-15T12:00:00.000000000Z` | `secs`, `nanos` | `Timestamp` |
| `DateTime<FixedOffset>` | `2026-01-15T17:30:00.000000000+05:30` | `secs`, `nanos`, `offset` | `Zoned` with a fixed offset |
| `DateTime<Tz>` (navree-time's `Tz`) | `2026-01-15T17:30:00.000000000+05:30[Asia/Kolkata]` | `secs`, `nanos`, `offset`, `zone` | `Zoned` |
| `NaiveDate` | `2026-01-15` | `year`, `month`, `day` | `Date` |
| `NaiveTime` | `09:30:00.000000000` | `hour`, `minute`, `second`, `nanos` | `Time` |
| `NaiveDateTime` | `2026-01-15T09:30:00.000000000` | the seven above | `DateTime` |
| `TimeDelta` | `-0.500000000s` | `secs`, `nanos` | `Span` |
| `Weekday` | `Mon` | `from_monday` (0 to 6) | `Weekday` |
| `Month` | `January` | `number` (1 to 12) | `Month` |
| `FixedOffset` | `+05:30` | `offset` | `Offset` |

- `secs` is whole seconds since 1970-01-01T00:00:00Z (negative before it),
  `nanos` the nanoseconds above them (0 to 999,999,999), `offset` seconds
  east of UTC, `zone` an IANA name.
- Canonical text always has nine fraction digits. A year outside 0 to 9999
  has a sign and at least four digits (`+262142`, `-0001`). An offset shows
  seconds only when it has any (`+05:53:28`).
- An instant's canonical text is worked out from `secs` and `offset` alone.
  At the two ends of the range it can therefore show a local year that no
  date holds (`+262143`, `-262144`): the instant is in range, its local time
  is not.
- A `TimeDelta`'s `secs` is rounded down and its `nanos` is 0 to
  999,999,999 above that: half a second before zero is `secs` -1, `nanos`
  500000000. Those are the two numbers its JSON holds.
- A leap second is kept today as second 59 with `nanos` of 1,000,000,000 or
  more. Such a line has `"leap":true`, its canonical text shows second 60,
  and its `secs` is the second before. `navree-datetime` keeps no leap
  second, so these are the lines where it cannot hold the same value; what
  it does with second 60 is its own rule, in its own docs. Every other line
  is a value it holds exactly.

## The files

| File | Lines | What |
|---|---|---|
| `display.jsonl` | 8960 | `Display` (`out`) and `Debug` (`debug`) of every value of the sweep. The first line is the largest instant, the one "never expires" is stored as. |
| `rfc3339.jsonl` | 4676 | `to_rfc3339()`, and `to_rfc3339_opts` for each way of writing the seconds with and without `Z`. |
| `rfc2822.jsonl` | 1876 | `to_rfc2822()`. |
| `serde.jsonl` | 6965 | The JSON document `serde_json::to_string` writes. |
| `strftime.jsonl` | 23000 | `format(pattern)`. |
| `patterns.jsonl` | 693 | Which patterns are taken and which are refused. |
| `parse_accept.jsonl` | 3672 | Texts that are read, with the value each is read as. |
| `parse_refuse.jsonl` | 14265 | Texts that are refused, with why. |
| `local.jsonl` | 2995 | Wall-clock times and days in a zone around its changes of offset: the instants they are. |
| `sqlite.jsonl` | 6910 | The text SQLx binds for SQLite. |
| `postgres.jsonl` | 5314 | The bytes SQLx writes for PostgreSQL. |
| `mysql.jsonl` | 5144 | The bytes SQLx writes for MySQL. |

### `display.jsonl`

`out` is `Display`, `debug` is `Debug` (`debug_err` if it failed). A `Month`
has no `Display`: its line has `name` and `debug`.

A line with `spec` holds what `format!` with that format string writes for
the value (`{:>40}`, `{:.3}`, `{:#?}` and so on): which types pad to a width
and which ignore it.

### `rfc3339.jsonl`

`call` is `to_rfc3339` or `to_rfc3339_opts`; for the second, `seconds` is
`secs`, `millis`, `micros`, `nanos` or `auto` (chrono's `SecondsFormat`) and
`z` is `use_z`. The month ends have `to_rfc3339` only, for UTC.

### `serde.jsonl`

`out` is the JSON document, quotes and all: `"\"2026-01-15T12:00:00Z\""` is
the text `"2026-01-15T12:00:00Z"`, and a `TimeDelta`'s is `[secs,nanos]`.

### `strftime.jsonl`

`pattern` is the pattern. Each specifier of chrono's documented vocabulary
is there alone, and each one that writes a single number also with `-`, `_`
and `0`. The date specifiers meet every day of the values below, the time
specifiers every time of day, and all of them a set of dates and times and
of instants that covers the range. The longer patterns (the ones Navree's
own code uses, the ones SQLx uses for database text, and some mixed ones)
are recorded for fewer values. Every month end has one pattern of all the
date specifiers.

A pattern a type cannot write (a day has no hour, a date and time has no
offset, `%#z` is for reading only) has one line per type, with
`"err":"format"`: it fails for every value of the type.

A line with `spec` holds what `format!` with that format string writes for
`value.format(pattern)`, which pads and truncates like a string.

### `patterns.jsonl`

`{"type":"pattern","value":<pattern>,"out":"ok"}` or `"out":"BadFormat"`.
Every pattern used in any file is here, with every ASCII character after
`%`, after each modifier and after `#`, every digit count before `f`, and
more patterns at the edge of the vocabulary. Formatting with a refused
pattern fails, and reading with one is refused as `BadFormat`.

### `parse_accept.jsonl` and `parse_refuse.jsonl`

`type` is the type that was asked for, `value` the text, and `via` the way
it was read:

| `via` | Call |
|---|---|
| `from_str` | `text.parse::<type>()` |
| `rfc3339` | `DateTime::parse_from_rfc3339(text)` |
| `rfc2822` | `DateTime::parse_from_rfc2822(text)` |
| `pattern` | `parse_from_str(text, pattern)` of the type, with `pattern` in the line |
| `serde` | `serde_json::from_str::<type>(text)`: here `value` is the JSON document |

A text is in exactly one of the two files for each `type`, `via` and
`pattern`. In `parse_accept.jsonl`, `out` is the canonical text of the value
and its numbers follow. In `parse_refuse.jsonl`, `out` is chrono's kind of
error (`Invalid`, `TooShort`, `TooLong`, `OutOfRange`, `Impossible`,
`NotEnough`, `BadFormat`), or `ParseWeekdayError`, `ParseMonthError`, or
`serde` with serde_json's `message` for any other refusal of a JSON
document (one that is not a text, a name that is no day of the week or
month, a span out of bounds).

Every text for an instant, a date and time, a day and a time of day goes
through `rfc3339` and through `from_str` and `serde` of every type, so the
files say what each makes of a text meant for another. Every pattern text
goes through `parse_from_str` of four types (`DateTime<FixedOffset>`,
`NaiveDateTime`, `NaiveDate`, `NaiveTime`): which of them fails, and how, is
what code that tries one after the other depends on. Besides the texts
written by hand,
every pattern reads back what it wrote for seven instants. And what the
databases hold as text for six instants (the largest and the smallest among
them) goes through every way such text is read today: `from_str`,
`rfc3339`, and the patterns navree-db and SQLx try in turn.

### `local.jsonl`

Not chrono alone: navree-time's zones, and its rule for a wall-clock time
that does not exist or comes twice.

- `"type":"LocalResult"`: `zone`, a wall-clock date and time (`value` and
  its numbers), and `out`: `none` (the clocks skipped it), `single`, or
  `ambiguous` (it comes twice). `secs` and `offset` are the instant, or the
  earlier of the two; `later_secs` and `later_offset` the later. `to_utc` is
  the instant `Zone::to_utc` gives today: the earlier of two, and a skipped
  time moved forward by the length of the gap.
- `"type":"LocalDay"`: `zone`, a day, and `start` and `end`: the first
  instant of that day in the zone and the first instant of the next
  (`LocalDay::utc_bounds`). Where the clocks skip midnight, `start` is the
  first instant that exists that day.

The times are the second before each skipped or repeated span, its first
second, its middle, its last second and the second after; the days are the
ones around each change. The changes are those of 1970 to 1972, of 2016 to
2030 and of 2037 to 2039 (each side of where seconds since the epoch pass
32 bits) in the six zones of the sweep and in America/Santiago,
Asia/Beirut, Europe/Dublin, America/Sao_Paulo and America/St_Johns. Each
zone also has two ordinary noons and their days.

### `sqlite.jsonl`, `postgres.jsonl`, `mysql.jsonl`

For each type, first a line without `value`: `type_info` is the SQL type
name SQLx reports (`TIMESTAMPTZ`, `DATETIME`), which prepared statements
are keyed by; `compatible` lists the column types the type says it can be
read from (among those of `bool`, the integers, the floats, `String`,
`Vec<u8>` and the date and time types); for PostgreSQL, `array_type_info`
is the array type's name.

Then one line per value, each made by calling SQLx's own `Encode` for the
type, with no database:

- `postgres.jsonl`: `bytes` is the hex of what `encode_by_ref` appends to a
  `PgArgumentBuffer`: the value in PostgreSQL's binary format, without the
  four-byte length a bind message puts before it.
- `mysql.jsonl`: `bytes` is the hex of what `encode_by_ref` appends to the
  buffer: the value in MySQL's binary protocol, its length byte first.
  MySQL has an `Encode` for `DateTime<Utc>` (a `TIMESTAMP`) and none for
  another offset; Navree binds an instant as the `NaiveDateTime` of its UTC
  time (a `DATETIME`), which is in the file too.
- `sqlite.jsonl`: `out` is the text bound. `SqliteArgumentsBuffer` shows
  what it holds only through `Debug`, so the text is taken from that; the
  program also works it out with the expression SQLx's source uses and
  stops if the two differ.

Between SQLx 0.9.0 and 0.9.1 the code that writes these types did not
change. The code that reads them did, in three places: PostgreSQL's `DATE`,
`TIMESTAMP` and `TIMESTAMPTZ` beyond the range (and `infinity`) became an
error instead of a panic; PostgreSQL's `TIME` of `24:00:00` became an error
instead of midnight; and a SQLite date and time held as a Julian day number
before 1970 keeps its fraction on the right side.

## The values

Every form is recorded for:

- the largest and smallest values, the epoch and the instants beside it;
- fractions of one to nine digits, and of the smallest and largest size;
- 28 February and 1 March 1900 and 2100, 29 February 2000 and 2024;
- years 0, 1, 5, 99, 100, 999, 1000, 9999, 10000, 25599, 25600 and 99999,
  and years before 0;
- the hours where a 12-hour clock turns, weeks at the turn of a year, each
  day of a week and each month;
- seconds since the epoch at the ends of 32 bits and at round numbers;
- four leap seconds;
- for an offset: none, a few seconds either way, 29 and 30 seconds (where
  a minute rounds), whole, half and three-quarter hours, the local mean
  times of Kolkata, New York and London, and one second short of a day
  either way;
- for a zone: a winter day, a summer day and a day of local mean time in
  each of eleven zones, each side of every change of 2026, and the epoch
  and the ends of the range in three of them.

And, for the forms that are stored or sent:

- the last day of every month from 1969 to 2101 (1,596 days), each at one
  of thirteen times of day and, with an offset, one of eleven offsets;
- the second before and the first second of every change of offset or
  abbreviation from 1800 to 2040 in Asia/Kolkata, America/New_York,
  Europe/London, Australia/Lord_Howe, Pacific/Chatham and America/Havana
  (in `display.jsonl`).

## Easy to miss

What the files show that is not what one would guess. Each line here can be
looked up in the file named.

Writing:

- An offset with seconds is written in full by `Display` (`+05:53:28`) and
  rounded to the nearest minute by RFC 3339, RFC 2822, JSON and `%z`
  (`+05:53`; 30 seconds is `+00:01`; one second short of a day is `+24:00`,
  which is then refused when read). Such a value does not come back from
  its own text as the same instant. `%:::z` drops the minutes without
  rounding. (`rfc3339.jsonl`, `strftime.jsonl`)
- `Z` is written only for an offset of exactly zero: 29 seconds is
  `+00:00` even where `Z` is asked for. A zoned value at zero (London in
  winter) is `Z` in JSON. (`rfc3339.jsonl`, `serde.jsonl`)
- At the ends of the range a local time can lie outside it. `Display`,
  `Debug`, `to_rfc3339()` and `format` write it (`+262143-01-01`);
  `to_rfc3339_opts`, JSON and the SQLite text panic. (`rfc3339.jsonl`,
  `serde.jsonl`, `sqlite.jsonl`)
- `to_rfc2822()` panics outside years 0 to 9999, and writes the day
  without a leading zero (`Thu, 1 Jan 1970`). (`rfc2822.jsonl`)
- A date and time without a zone has a space in `Display`
  (`2026-01-15 09:30:00`) and a `T` in `Debug` and in JSON. A zoned value's
  `Debug` has no space before the abbreviation (`2026-01-15T17:30:00IST`).
  (`display.jsonl`)
- With `Display`, only `Weekday` and `value.format(..)` pad to a width;
  every other type ignores width, fill and precision. (`display.jsonl`,
  `strftime.jsonl`)
- A span is `P0D` when zero, else seconds only: `PT90S`, `PT1.5S`,
  `-PT0.5S`. Its `Debug` is the derived one and names the type:
  `TimeDelta { secs: 1, nanos: 500000000 }`. (`display.jsonl`)
- `%C` is the year divided by 100 cut to eight bits, so outside 0 to 9999
  it writes other characters (`:0` for year 10000, `I5` for year -1).
  (`strftime.jsonl`)
- `%Y` from 1000 to 9999 ignores its modifier; outside 0 to 9999 it always
  has a sign. `%q`, `%u` and `%w` take a modifier and ignore it.
  (`strftime.jsonl`)
- `%f` is the nanoseconds as a number of nine digits, not a fraction:
  `%-f` of half a second is `500000000`. `%.f` writes nothing, three, six
  or nine digits. `%s` has no padding, `%0s` pads to nine.
  (`strftime.jsonl`)
- `%Z` is the abbreviation for a zone (`IST`), `UTC` for UTC and the offset
  for a fixed offset (`+05:30`). (`strftime.jsonl`)

Reading:

- A comma before the fraction is refused everywhere. So is an offset of
  hours alone (`+05`), except by `%#z`.
- `parse_from_rfc3339` takes `t`, `z` and a space for the `T`; it refuses
  an offset without a colon, a missing zero and anything not in its place.
  `FromStr` of an instant is looser: spaces between the parts, numbers of
  one digit, `+0530`, `UTC` in any case, a year with a sign.
- `FromStr` of a date and time without a zone wants a capital `T` and the
  seconds: it refuses what its own `Display` writes. `FromStr` of a time of
  day takes `09:30`.
- Second 60 is accepted in any minute, and is a leap second. Hour 24 is
  refused.
- The minus sign U+2212 is accepted in an offset, except in RFC 2822.
- In a pattern, white space matches any white space or none
  (`15/01/202609:30` is read by `%d/%m/%Y %H:%M`), and a number may have
  white space before it. Numbers may be shorter than their width, never
  longer: `%Y` without a sign reads at most four digits.
- `%y` is 1970 to 2069. A two-digit year in RFC 2822 is 1950 to 2049.
- `%f` reads `5` as five nanoseconds. `%.f`, `%.3f`, `%.6f` and `%.9f` may
  be absent; after a dot, `%.3f` wants exactly three digits and `%.f` at
  least one.
- `%Z` skips to the next white space and gives no offset. `%z`, `%:z`,
  `%::z` and `%:::z` all read the same texts (colon or not, never `Z`,
  never hours alone); `%#z` also reads `Z`, `z` and hours alone.
- `parse_from_str` of an instant needs an offset or `%s` in the pattern, of
  a date and time needs both, of a day ignores the time fields and of a
  time of day ignores the date fields, but the whole text must still fit
  the whole pattern. Fields that disagree (a wrong day of the week, `%p`
  against a 24-hour `%H`, `%s` against the date) are `Impossible`.
- `FromStr` of an offset stops after the minutes and ignores what follows.
- JSON for an instant, a day or a time goes through `FromStr`, so it takes
  everything `FromStr` takes.

Databases:

- PostgreSQL and MySQL are sent microseconds: the last three digits of the
  nanoseconds are cut, not rounded.
- MySQL's bytes are shorter when the time is midnight or has no fraction.
  A year below 0 or above 65535 is an error, also for the largest instant.
- SQLite holds an instant as RFC 3339 with `+00:00`, never `Z`, and a date
  and time with a space. The texts are compared as text in SQL, so every
  byte counts.
