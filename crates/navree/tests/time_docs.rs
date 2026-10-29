//! The examples of `content/docs/digging-deeper/time-zones.mdx`, written as
//! an app writes them (the facade, the prelude and the macros), compiled
//! and run.
#![cfg(all(
    feature = "time",
    feature = "schedule",
    feature = "testing",
    feature = "validation"
))]

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use navree::contracts::{
    BoxFuture, Clock, Dispatcher, FixedClock, Job, JobContext, JobEnvelope, ZoneLevel, ZoneOwner,
    ZoneRequest, ZoneSource,
};
use navree::db::{Db, Executor, Statement};
use navree::kernel::{CommandSpec, Resolver};
use navree::prelude::*;
use navree::schedule::{LocalRun, RunStatus, Scheduler};
use navree::testing::{TestApp, principal};
use navree::time::{FixedHolidays, Holidays, LocalDayError, ZoneError, Zones, ZonesFake};
use sea_orm::sea_query::{self, ColumnDef, Expr, ExprTrait, Table};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

// --- Zones ------------------------------------------------------------------

#[test]
fn zones_are_checked_and_normalized() -> Result<(), ZoneError> {
    let zone = Zone::parse("Asia/Calcutta")?;
    assert_eq!(zone.name(), "Asia/Kolkata");
    assert_eq!(Zone::parse("us/eastern")?.name(), "America/New_York");
    assert_eq!(Zone::parse("Etc/UTC")?.name(), "UTC");
    assert!(Zone::parse("Mars/Olympus_Mons").is_err());
    assert_eq!(Zone::for_country("IN"), Some(zone));
    assert_eq!(zone.countries(), ["IN"]);
    Ok(())
}

// --- The request's zone and report windows -----------------------------------

/// `signups(id, created_at)`.
mod signup {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "signups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Creates `signups` and `users(id, timezone)`.
struct CreateTables;

impl MigrationName for CreateTables {
    fn name(&self) -> &str {
        "m20261027_000001_create_time_docs_tables"
    }
}

#[async_trait]
impl MigrationTrait for CreateTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("signups")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(
                        ColumnDef::new("created_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("users")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("timezone").string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(Serialize)]
struct SignupReport {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    signups: u64,
}

/// Reports counted in the request's zone.
#[injectable]
pub struct ReportsController;

#[controller("/")]
impl ReportsController {
    #[get("/zone")]
    #[authenticated]
    async fn zone(&self, zone: CurrentZone) -> Json<serde_json::Value> {
        Json(json!({ "zone": zone.name(), "today": zone.today() }))
    }

    #[get("/reports/signups")]
    #[authenticated]
    async fn signups(
        &self,
        zone: CurrentZone,
        Valid(Query(range)): Valid<Query<DayRange>>,
        uow: Uow,
    ) -> Result<WithZone<SignupReport>> {
        let (from, to) = range.utc_bounds(&zone);
        let signups = signup::Entity::find()
            .filter(signup::Column::CreatedAt.gte(from))
            .filter(signup::Column::CreatedAt.lt(to))
            .count(&*uow.tx().await?)
            .await?;
        Ok(WithZone::new(&zone, SignupReport { from, to, signups }))
    }
}

/// The zone of each user, from `users.timezone`.
pub struct UserZones {
    db: Arc<Db>,
}

impl UserZones {
    async fn find(&self, id: Uuid) -> Result<Option<ZoneOwner>, BoxError> {
        let query = sea_query::Query::select()
            .column("timezone")
            .from("users")
            .and_where(Expr::col("id").eq(id))
            .to_owned();
        Ok(match self.db.conn().query_one(&query).await? {
            Some(row) => {
                let zone: Option<String> = row.try_get("", "timezone")?;
                Some(ZoneOwner {
                    id,
                    zone,
                    country: None,
                })
            }
            None => None,
        })
    }
}

impl ZoneSource for UserZones {
    fn level(&self) -> ZoneLevel {
        ZoneLevel::User
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            match request.user_id() {
                Some(id) => self.find(id).await,
                None => Ok(None),
            }
        })
    }

    fn zone_of(&self, owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move { self.find(owner).await })
    }
}

/// The reports feature: routes, tables and the users' zones.
struct ReportsModule;

impl Module for ReportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ReportsController>()
            .migrations(MigrationSet::new("time_docs").migration(CreateTables));
        m.multi(Factory::<dyn ZoneSource>::new(|r: Resolver| async move {
            Ok(Arc::new(UserZones {
                db: r.get::<Db>().await?,
            }) as Arc<dyn ZoneSource>)
        }));
    }
}

async fn reports_app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(DatabaseModule::from_config())
        .module(ValidationModule::for_root())
        .module(TimeModule::from_config())
        .module(ReportsModule)
        .build()
        .await
}

async fn add_user(app: &TestApp, timezone: Option<&str>) -> Uuid {
    let id = Uuid::now_v7();
    let insert = sea_query::Query::insert()
        .into_table("users")
        .columns(["id", "timezone"])
        .values_panic([id.into(), timezone.map(str::to_owned).into()])
        .to_owned();
    app.get::<Db>().await.conn().execute(&insert).await.unwrap();
    id
}

async fn add_signup(app: &TestApp, at: &str) {
    let created_at: DateTime<Utc> = at.parse().unwrap();
    signup::Entity::insert(signup::ActiveModel {
        id: Set(Uuid::now_v7()),
        created_at: Set(created_at),
    })
    .exec(app.get::<Db>().await.conn())
    .await
    .unwrap();
}

#[tokio::test]
async fn a_user_source_gives_the_request_its_zone() {
    let app = reports_app().await;
    let kolkata = add_user(&app, Some("Asia/Calcutta")).await;
    let unset = add_user(&app, None).await;
    app.acting_as(principal(kolkata))
        .get("/api/zone")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("zone", json!("Asia/Kolkata"));
    app.acting_as(principal(unset))
        .get("/api/zone")
        .send()
        .await
        .assert_json_path("zone", json!("UTC"));
}

#[tokio::test]
async fn reports_count_local_days_and_echo_the_zone() {
    let app = reports_app().await;
    let user = add_user(&app, Some("Asia/Kolkata")).await;
    add_signup(&app, "2026-09-30T18:29:59Z").await; // 30 September in Kolkata
    add_signup(&app, "2026-09-30T18:30:00Z").await; // 1 October
    add_signup(&app, "2026-10-31T18:29:59Z").await; // 31 October
    add_signup(&app, "2026-10-31T18:30:00Z").await; // 1 November
    let body = app
        .acting_as(principal(user))
        .get("/api/reports/signups")
        .query(&[("from", "2026-10-01"), ("to", "2026-10-31")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "from": "2026-09-30T18:30:00Z",
            "to": "2026-10-31T18:30:00Z",
            "signups": 2,
            "timeZone": "Asia/Kolkata",
        })
    );
    app.acting_as(principal(user))
        .get("/api/reports/signups")
        .query(&[("from", "2026-10-31"), ("to", "2026-10-01")])
        .send()
        .await
        .assert_validation_error("to");
    app.acting_as(principal(user))
        .get("/api/reports/signups")
        .query(&[("from", "2026-1-5"), ("to", "2026-10-01")])
        .send()
        .await
        .assert_status(400);
}

// --- Reports in SQL ------------------------------------------------------------

#[tokio::test]
async fn sql_groups_rows_by_local_day() {
    let app = reports_app().await;
    for at in [
        "2026-03-28T23:30:00Z", // 28 March, 23:30 GMT
        "2026-03-29T22:30:00Z", // 29 March, 23:30 BST
        "2026-03-29T23:30:00Z", // 30 March, 00:30 BST
    ] {
        add_signup(&app, at).await;
    }
    let db = app.get::<Db>().await;
    let zone = Zone::parse("Europe/London").unwrap();
    let range =
        DayRange::new("2026-03-28".parse().unwrap(), "2026-03-30".parse().unwrap()).unwrap();

    let (from, to) = range.utc_bounds(&zone);
    let sql = zone.sql(Executor::backend(&*db)).within(from, to);
    let day = sql.local_date(&sql.column("created_at"));
    let statement = Statement::portable(
        format!(
            "SELECT {day} AS day, COUNT(*) AS signups FROM signups \
             WHERE created_at >= ? AND created_at < ? GROUP BY {day}",
            day = day.sql()
        ),
        [from.into(), to.into()],
    );

    let mut rows: Vec<(String, i64)> = db
        .fetch_all(&statement)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.get::<String>("day").unwrap(),
                row.get::<i64>("signups").unwrap(),
            )
        })
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            ("2026-03-28".to_owned(), 1),
            ("2026-03-29".to_owned(), 1),
            ("2026-03-30".to_owned(), 1),
        ]
    );
}

// --- Send windows and holidays ---------------------------------------------------

/// A digest for one workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendDigest {
    /// The workspace.
    pub workspace_id: Uuid,
}

impl Job for SendDigest {
    const NAME: &'static str = "time_docs.send_digest";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

/// Keeps what is dispatched.
#[derive(Default)]
struct Recorder(Mutex<Vec<JobEnvelope>>);

impl Dispatcher for Recorder {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        self.0.lock().unwrap().push(env);
        Box::pin(async { Ok("1".to_owned()) })
    }
}

/// Queues the digest of `workspace_id` for its next working hour.
async fn queue_digest(
    zones: &Zones,
    dispatcher: &dyn Dispatcher,
    workspace_id: Uuid,
) -> Result<(), BoxError> {
    let window = SendWindow::weekdays()
        .between("09:00", "17:00")
        .except(zones.holidays());
    let zone = zones.zone_of(ZoneLevel::Workspace, workspace_id).await?;
    let delay = window.delay(zones.now(), &zone);
    dispatcher
        .dispatch_later(SendDigest { workspace_id }, delay)
        .await?;
    Ok(())
}

#[tokio::test]
async fn digests_wait_for_the_workspaces_working_hours() {
    let workspace = Uuid::now_v7();
    let workspaces =
        ZonesFake::new(ZoneLevel::Workspace).owner(ZoneOwner::new(workspace).zone("Asia/Kolkata"));
    // Friday 30 October 2026, 17:30 in Kolkata.
    let clock = Arc::new(FixedClock::new("2026-10-30T12:00:00Z".parse().unwrap()));
    let app = TestApp::builder()
        .module(TimeModule::from_config())
        .module(workspaces.module())
        .override_value::<dyn Clock>(clock)
        .build()
        .await;
    let zones = app.get::<Zones>().await;
    let recorder = Recorder::default();
    queue_digest(&zones, &recorder, workspace).await.unwrap();
    let queued = recorder.0.lock().unwrap();
    // Monday 2 November, 09:00 in Kolkata: 2 days and 15.5 hours later.
    let expected = Duration::from_secs(2 * 86_400 + 15 * 3600 + 1800);
    assert_eq!(queued[0].delay_secs, Some(expected.as_secs()));
}

#[test]
fn holidays_by_country_and_extra_days() -> Result<(), LocalDayError> {
    let gb = Holidays::for_country("GB");
    assert!(gb.is_holiday("2026-12-28".parse()?)); // Boxing Day (substitute day)
    let december = gb.between(&DayRange::month_of("2026-12-01".parse()?));
    assert_eq!(december.len(), 2);

    let closures = Holidays::for_country("IN").with_days(["2026-11-08".parse()?]);
    assert!(closures.is_holiday("2026-11-08".parse()?));

    let calendar = FixedHolidays::new()
        .add("NP", "2026-10-21".parse()?, "Dashain")
        .add("NP", "2026-11-08".parse()?, "Tihar");
    let nepal = Holidays::with_calendar("NP", calendar);
    assert_eq!(nepal.in_year(2026).len(), 2);
    Ok(())
}

// --- Scheduling in local time ---------------------------------------------------

/// Schedules on local clocks.
struct DigestsModule;

impl Module for DigestsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        for name in ["reports:close-day", "crm:task-reminders"] {
            m.command(CommandSpec::new(name, "test", |_ctx, _args| async {
                Ok(())
            }));
        }
        m.schedule(|s| {
            // 02:00 in London, all year.
            s.command("reports:close-day", &[])
                .daily_at("02:00")
                .in_zone("Europe/London");
            // 08:00 on the app's default zone (time.default_zone).
            s.command("crm:task-reminders", &[])
                .daily_at("08:00")
                .in_default_zone();
            // 09:00 in each workspace's own zone, one run per workspace.
            s.job_with(|run: &LocalRun| SendDigest {
                workspace_id: run.owner.unwrap_or_default(),
            })
            .daily_at("09:00")
            .per_workspace_local();
        });
    }
}

#[tokio::test]
async fn schedules_run_on_local_clocks() {
    let workspace = Uuid::now_v7();
    let workspaces =
        ZonesFake::new(ZoneLevel::Workspace).owner(ZoneOwner::new(workspace).zone("Asia/Kolkata"));
    let recorder = Arc::new(Recorder::default());
    let app = TestApp::builder()
        .temp_database()
        .config("time.default_zone", "America/New_York")
        .module(DatabaseModule::from_config())
        .module(ScheduleModule::for_root())
        .module(TimeModule::from_config())
        .module(workspaces.module())
        .module(DigestsModule)
        .override_value::<dyn Dispatcher>(recorder.clone())
        .build()
        .await;
    let scheduler = app.get::<Scheduler>().await;
    let at = |text: &str| text.parse::<DateTime<Utc>>().unwrap();
    // 26 October 2026: London is on GMT, New York on EDT.
    let london = scheduler.tick(at("2026-10-26T02:00:00Z")).await.unwrap();
    assert_eq!(london[0].task, "reports:close-day");
    let new_york = scheduler.tick(at("2026-10-26T12:00:00Z")).await.unwrap();
    assert_eq!(new_york[0].task, "crm:task-reminders");
    let kolkata = scheduler.tick(at("2026-10-26T03:30:00Z")).await.unwrap();
    assert_eq!(
        kolkata[0].task,
        format!("time_docs.send_digest@{workspace}")
    );
    assert_eq!(kolkata[0].status, RunStatus::Succeeded);
    let queued = recorder.0.lock().unwrap();
    assert_eq!(queued[0].payload["workspace_id"], json!(workspace));
}

// --- Validation -----------------------------------------------------------------

/// A profile update keeping the zone as a string.
#[derive(Deserialize, garde::Validate)]
pub struct UpdateProfile {
    /// The user's IANA time zone.
    #[garde(custom(navree::time::rules::time_zone))]
    pub timezone: Option<String>,
}

#[test]
fn the_rule_checks_zone_names() {
    use garde::Validate;
    let ok = UpdateProfile {
        timezone: Some("Asia/Calcutta".into()),
    };
    assert!(ok.validate().is_ok());
    let unset = UpdateProfile { timezone: None };
    assert!(unset.validate().is_ok());
    let bad = UpdateProfile {
        timezone: Some("Mars/Base".into()),
    };
    assert!(bad.validate().is_err());
}

// --- Testing ----------------------------------------------------------------------

#[tokio::test]
async fn the_report_uses_the_users_zone() {
    let user = Uuid::now_v7();
    let users = ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(user).zone("Asia/Kolkata"));
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(TimeModule::from_config())
        .module(users.module())
        .module(ProbeModule)
        .build()
        .await;
    app.acting_as(principal(user))
        .get("/api/zone")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("zone", json!("Asia/Kolkata"));
}

/// `GET /api/zone` alone (the testing example's module).
struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ReportsController>();
    }
}

// --- Zones outside a request --------------------------------------------------------

#[tokio::test]
async fn jobs_resolve_zones_with_the_service() {
    let workspace_id = Uuid::now_v7();
    let workspaces =
        ZonesFake::new(ZoneLevel::Workspace).owner(ZoneOwner::new(workspace_id).country("AE"));
    let app = TestApp::builder()
        .module(TimeModule::from_config())
        .module(workspaces.module())
        .build()
        .await;
    let ctx = app.ctx();

    let zones = ctx.get::<Zones>().await.unwrap();
    let zone = zones
        .zone_of(ZoneLevel::Workspace, workspace_id)
        .await
        .unwrap();
    let yesterday = DayRange::single(zone.today(zones.now()).pred());

    assert_eq!(zone.name(), "Asia/Dubai");
    assert_eq!(yesterday.day_count(), 1);
}
