---
name: navree-time-zones
description: "Counts dates, reports and messages on the clock of the person or workspace they are for, with `TimeModule` (navree-time): checked IANA `Zone`s with PostgreSQL-safe names, the request's zone (`CurrentZone`: user, workspace, organization, default), `LocalDay` and `DayRange` turned into UTC bounds, `WithZone` responses, `SendWindow` working hours with weekends and `Holidays`, `AT TIME ZONE` SQL with a checked zone, schedules with `in_zone` and `per_workspace_local`, workspace and organization `timezone`/`country`, `ZonesFake` in tests. Use when a report counts days, a reminder must arrive in working hours, a cron task should run at local time or per workspace, a user or workspace picks a time zone, or a zone name breaks PostgreSQL."
---

# Time zones

Timestamps are stored in UTC. `TimeModule` (facade feature `time`, in
`full`) gives each request and job the zone it belongs to and converts
local days and times to the UTC instants queries filter on.

## Steps

1. Add `TimeModule::from_config()` to the app root (section `time`).
2. Set `time.default_zone` (`APP_TIME__DEFAULT_ZONE`), and
   `time.holidays.country` if the app observes a country's holidays.
3. Give zones a source: `WorkspaceModule` already registers the workspace
   and organization levels (`timezone` and `country` on `workspaces` and
   `organizations`; the organization level answers once
   `workspace.organizations.enabled`); register users' zones with a
   `dyn ZoneSource` of your own.
4. Take `CurrentZone` in handlers that count days; return `WithZone`.
5. Test with `ZonesFake` and a `FixedClock`.

```toml
[time]
default_zone = "Europe/London"
routes = true                 # GET /api/time/zones (public)

[time.holidays]
country = "GB"
days = ["2026-12-24"]         # or "2026-12-24,2026-12-31" from the environment
```

An unknown zone or country fails the boot naming the key.

## Zones

```rust
use navree::time::Zone;

let zone = Zone::parse("Asia/Calcutta")?;          // any case, trimmed
assert_eq!(zone.name(), "Asia/Kolkata");           // legacy alias replaced
let gulf = Zone::for_country("AE").unwrap();       // Asia/Dubai
let label = zone.offset_label(now);                // "+05:30"
let today = zone.today(now);                       // LocalDay
let at = zone.to_utc(naive_local);                 // gap: moves forward; repeat: earlier
```

- Store `zone.name()`, never the raw input: PostgreSQL servers without the
  tz backward links reject `Asia/Calcutta`, `US/Eastern`, `Europe/Kiev`.
- A field of type `Zone` deserializes through `Zone::parse` (400 on the
  field when unknown). For `String` fields use
  `#[validate(custom(navree::time::rules::time_zone))]`.

## The request's zone

```rust
#[get("/reports/signups")]
#[authenticated]
async fn signups(
    &self,
    zone: CurrentZone,
    Valid(Query(range)): Valid<Query<DayRange>>,   // ?from=2026-10-01&to=2026-10-31
    uow: Uow,
) -> Result<WithZone<SignupReport>> {
    let (from, to) = range.utc_bounds(&zone);       // [start of from, end of to)
    let signups = Signup::query()
        .where_(Signup::CREATED_AT.gte(from))
        .where_(Signup::CREATED_AT.lt(to))
        .count(&uow)
        .await?;
    Ok(WithZone::new(&zone, SignupReport { from, to, signups }))   // adds "timeZone"
}
```

- Resolution: user source, then the request's workspace (same resolvers as
  `CurrentWorkspace`; members only), then organization (the workspace's,
  or the one an organization request names, members only), then
  `time.default_zone`. Per level: stored zone, else the country's primary
  zone. A stored name that isn't a zone is skipped with a warning.
- `zone.level()`, `zone.owner()`, `zone.from_country()`, `zone.today()`,
  `zone.now()` (app clock). Without `TimeModule` the extractor answers 500.
- Jobs: `ctx.get::<Zones>().await?` then `zone_of(ZoneLevel::Workspace, id)`,
  `owners(level)`, `holidays()`, `now()`. A workspace with neither zone nor
  country gets its organization's there (and in `per_workspace_local`).

## Users' zones (a source)

```rust
impl ZoneSource for UserZones {
    fn level(&self) -> ZoneLevel { ZoneLevel::User }
    fn zone_of_request<'a>(&'a self, request: &'a mut ZoneRequest<'_>)
        -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            match request.user_id() { Some(id) => self.find(id).await, None => Ok(None) }
        })
    }
    fn zone_of(&self, owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move { self.find(owner).await })
    }
}

m.multi(Factory::<dyn ZoneSource>::new(|r: Resolver| async move {
    Ok(Arc::new(UserZones { db: r.get::<Db>().await? }) as Arc<dyn ZoneSource>)
}));
```

Return the raw stored values (`ZoneOwner { id, zone, country }`);
`TimeModule` validates them. Stateless sources: `m.zone_source(source)`.

## Reports in SQL

```rust
use navree::db::{Executor, Statement};

let sql = zone.sql(Executor::backend(&db)).within(from, to);
let day = sql.local_date(&sql.column("created_at"));   // a Statement fragment
let statement = Statement::portable(
    format!(
        "SELECT {day} AS day, COUNT(*) AS signups FROM signups \
         WHERE created_at >= ? AND created_at < ? GROUP BY {day}",
        day = day.sql()
    ),
    [from.into(), to.into()],
);
let rows = db.fetch_all(&statement).await?;
```

PostgreSQL: `AT TIME ZONE 'zone'`; MySQL: `CONVERT_TZ`; SQLite: offset in
force. Always pass `within(from, to)`: SQLite (and MySQL without tz tables)
then switches offsets at each DST change in the window. The zone is
inlined as a literal (safe: checked name), so the expression can repeat in
`GROUP BY` (carry the fragment's values once per place it appears).
`zone.sql(..)` takes `navree::db::Backend`: `Executor::backend(&db)`.

## Send windows and holidays

```rust
let window = SendWindow::weekdays()            // or working_days_in("SA"), on(&[..]), always()
    .between("09:00", "17:00")                 // panics on bad literals; try_between for input
    .except(zones.holidays());                 // Holidays::for_country("GB"), with_days([..])
let delay = window.delay(zones.now(), &zone);  // Duration::ZERO inside the window
dispatcher.dispatch_later(SendDigest { workspace_id }, delay).await?;
```

Built-in holiday rules: US, GB, FR, DE, CA, AU, IN, AE, SA (national days
only; lunar and one-off holidays are not computed). Add days with
`with_days`, or a `FixedHolidays` / own `HolidayCalendar`.

## Schedules on local clocks

```rust
m.schedule(|s| {
    s.command("reports:close-day", &[]).daily_at("02:00").in_zone("Europe/London");
    s.command("crm:task-reminders", &[]).daily_at("08:00").in_default_zone();
    s.job_with(|run: &LocalRun| SendDigest { workspace_id: run.owner.unwrap_or_default() })
        .daily_at("09:00")
        .per_workspace_local();               // needs TimeModule; one run per workspace
});
```

Runs are claimed as `<task>@<workspace id>` (task names at most 154
bytes). `LocalRun::current()` inside the run gives `owner`, `zone`,
`tick`, `local`.

## Workspaces and organizations

`POST /api/workspaces {name, timezone?, country?}` and
`PUT /api/workspaces/{workspace}/time-zone {timezone, country}`
(`workspace.update`); `workspaces.set_time_zone(&uow, id, Some("Asia/Kolkata"), Some("IN"))`.
Organizations: `POST /api/organizations` and `PATCH /api/organizations/{organization}`
take `timezone` / `country`; in code `organizations.update(&uow, id,
OrganizationChanges { timezone: Some(Some(..)), .. })`. Both store the
current zone name (`Asia/Calcutta` becomes `Asia/Kolkata`) and the
upper-case country; unknown values are 400s on `timezone` / `country`.
A workspace without either follows its organization's zone.

## Testing

```rust
let users = ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(user).zone("Asia/Kolkata"));
let workspaces = ZonesFake::new(ZoneLevel::Workspace)
    .header("X-Workspace")                    // request owner id from this header
    .owner(ZoneOwner::new(ws).country("AE"));
let app = TestApp::builder()
    .module(TimeModule::from_config())
    .module(users.module())
    .module(workspaces.module())
    .override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))
    .build()
    .await;
```

One fake per level (a second fails the boot). `fake.set(..)` and
`fake.remove(id)` change zones after boot.

## Common mistakes

- Filtering `created_at` by `NaiveDate` or by UTC midnight: use
  `range.utc_bounds(&zone)` (half-open; 23- or 25-hour days on DST changes).
- Storing a browser-reported zone as is: parse it with `Zone` first.
- `daily_at("09:00")` without `in_zone`/`per_workspace_local` is 09:00 UTC.
- SQLite `local_date` without `within`: uses today's offset for all rows.
- Using `s.job(value).per_workspace_local()`: every workspace gets the same
  value; build per workspace with `s.job_with(|run| ..)`.
