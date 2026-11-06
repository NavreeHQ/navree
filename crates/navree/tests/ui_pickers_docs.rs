//! The server-side examples of `content/docs/components/combobox.mdx`,
//! `content/docs/components/command.mdx` and `content/docs/ui/forms.mdx`, written as
//! an app writes them (the facade and its prelude), compiled and run: a searched list
//! answers from the database, a post that names someone the caller may not pick is
//! refused, and the form that comes back shows the chosen person's name.

use std::sync::Arc;

use http::Method;
use navree::db::{Db, Executor, Statement};
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::testing::TestApp;
use navree::ui::{OptionQuery, UiModule};
use serde::Serialize;

const FORM: &str = "application/x-www-form-urlencoded";

/// `pickers_people(id, team_id, name, email)`.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "pickers_people")]
pub struct Person {
    /// The key.
    pub id: i64,
    /// The team the person is in: the caller sees one team's people.
    pub team_id: i64,
    /// The name the list shows.
    pub name: String,
    /// The address the search also reads.
    pub email: String,
}

/// The people this caller may pick: the app's scoped query, which a real app builds from
/// the request (the workspace, the team, an access rule). Here it is team 1.
fn visible() -> navree::orm::Builder<Person> {
    Person::query().where_(navree::orm::col("team_id").eq(1_i64))
}

/// What `components/combobox.mdx` shows: the search and the check of what was picked.
mod combobox_page {
    use super::*;
    use http::request::Parts;
    use navree::http::response::{IntoResponse, Response};
    use navree::ui::form::{FieldErrors, Rejected};
    use navree::ui::*;

    const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);

    pub async fn people(query: OptionQuery, uow: Uow) -> Result<ChoiceList> {
        Ok(PEOPLE
            .find(visible(), &query, &uow, "/people/options", |record| {
                let person = record.into_inner();
                Choice::new(person.id.to_string(), person.name)
            })
            .await?)
    }

    #[derive(serde::Deserialize, Validate, Form)]
    pub struct NewDeal {
        /// Owner
        #[validate(skip)]
        #[form(widget = combobox, placeholder = "Search people")]
        owner_id: Option<String>,
    }

    pub async fn store(parts: Parts, uow: Uow, Submit(deal): Submit<NewDeal>) -> Result<Response> {
        let picked = deal.owner_id.iter();
        if !PEOPLE
            .all_pickable(visible(), picked, &uow, OptionSource::as_int)
            .await?
        {
            let errors = FieldErrors::from([(
                "owner_id".to_owned(),
                vec!["Choose someone from the list.".to_owned()],
            )]);
            return Ok(Rejected::after_decode(&deal, errors, &parts).into_response());
        }
        Ok("saved".into_response())
    }
}

/// What `components/command.mdx` shows: results from the server, as links.
mod command_page {
    use super::*;
    use navree::ui::*;

    const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);

    pub async fn palette_search(query: OptionQuery, uow: Uow) -> Result<ChoiceList> {
        // People whose name contains the text, as links
        let people = PEOPLE
            .find(visible(), &query, &uow, "/palette/search", |record| {
                let person = record.into_inner();
                Choice::new(person.id.to_string(), person.name)
                    .link(format!("/people/{}", person.id))
            })
            .await?;
        Ok(people)
    }
}

/// What `ui/forms.mdx` shows: the refused form, with the label of the person picked.
mod forms_page {
    use super::*;
    use navree::http::response::{IntoResponse, Response};
    use navree::ui::form::Rejected;
    use navree::ui::*;

    const PEOPLE: OptionSource = OptionSource::new("id", "name").search_in(&["name", "email"]);

    #[derive(serde::Deserialize, Validate, Form)]
    pub struct NewDeal {
        /// Title
        #[validate(length(min = 1))]
        pub title: String,
        /// Owner
        #[validate(skip)]
        #[form(widget = combobox, placeholder = "Search people")]
        pub owner_id: Option<String>,
    }

    pub async fn store(
        shell: AppShell,
        uow: Uow,
        submit: Result<Submit<NewDeal>, Rejected<NewDeal>>,
    ) -> Result<Response> {
        match submit {
            Ok(Submit(deal)) => Ok(Reply::redirect("/deals")
                .toast(ToastTone::Success, format!("{} created", deal.title))
                .into_response()),
            Err(rejected) => {
                // The labels of the people posted that this caller may pick, from the same
                // scoped query the search uses: someone else's name is never looked up.
                let owners = PEOPLE
                    .pickable(
                        visible(),
                        rejected.values_of("owner_id"),
                        &uow,
                        OptionSource::as_int,
                        |record| {
                            let person = record.into_inner();
                            Choice::new(person.id.to_string(), person.name)
                        },
                    )
                    .await?;
                Ok(rejected.choices("owner_id", owners).respond(|form| {
                    shell
                        .page("New deal")
                        .body(form.post("/deals").submit("Create deal"))
                }))
            }
        }
    }
}

/// What `components/date-range-picker.mdx` shows under "A report's window in the address":
/// the picker reads the window from the address, the limits hold the server to it, and
/// today is the person's own.
#[cfg(feature = "time")]
mod report_page {
    use super::*;
    use navree::http::Query;
    use navree::ui::dates::{DateLocale, DateRangePicker, DateRangeQuery, DateRules, RangeRules};
    use navree::ui::*;

    fn window_limits() -> RangeRules {
        RangeRules::new(DateRules::new()).min_days(2).max_days(90)
    }

    fn refused(shell: &AppShell, text: String) -> AppPage {
        shell
            .page("Report")
            .status(http::StatusCode::UNPROCESSABLE_ENTITY)
            .body(el("p").child(text))
    }

    pub async fn report(
        shell: AppShell,
        Query(query): Query<DateRangeQuery>,
        zone: CurrentZone, // navree-time: the person's or the workspace's zone
    ) -> Result<AppPage> {
        let limits = window_limits();
        let dates = DateLocale::of("en-IN");
        let range = match query.checked(&limits) {
            Ok(range) => range, // Option<DateRange>
            Err(why) => return Ok(refused(&shell, why.message(&dates))),
        };
        if let Some(range) = range {
            let (start, end) = range.utc_bounds(&zone.zone().tz()); // [start, end)
            assert!(start < end);
        }
        let picker = DateRangePicker::new("from", "to")
            .today_in(&zone)
            .locale(dates)
            .limits(limits)
            .value_opt(range);
        Ok(shell.page("Report").body(picker))
    }
}

struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<navree::kernel::ProviderKey> {
        Vec::new()
    }

    fn construct(
        _: Resolver,
    ) -> navree::contracts::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        #[allow(unused_mut)]
        let mut routes = vec![
            RouteDef::on(Method::GET, "!/people/options", combobox_page::people).public(),
            RouteDef::on(
                Method::GET,
                "!/palette/search",
                command_page::palette_search,
            )
            .public(),
            RouteDef::on(Method::POST, "!/picked", combobox_page::store).public(),
            RouteDef::on(Method::POST, "!/deals", forms_page::store).public(),
        ];
        #[cfg(feature = "time")]
        routes.push(RouteDef::on(Method::GET, "!/report", report_page::report).public());
        routes
    }
}

struct PickersModule;

impl Module for PickersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        // The app's one shell, which `AppShell` reads.
        m.factory(Factory::<navree::ui::Shell>::new(|_| async {
            Ok(Arc::new(navree::ui::Shell::new("Agra")))
        }))
        .exports::<navree::ui::Shell>();
        m.controller::<Pages>();
    }
}

async fn app() -> TestApp {
    let builder = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(UiModule)
        .module(PickersModule);
    #[cfg(feature = "time")]
    let builder = builder.module(TimeModule::from_config());
    let app = builder.build().await;
    let db = app.get::<Db>().await;
    db.execute(&Statement::text(
        "CREATE TABLE pickers_people (id INTEGER PRIMARY KEY, team_id INTEGER NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL)",
    ))
    .await
    .unwrap();
    // Gauri and Arun are in the caller's team; Anu is in another one.
    db.execute(&Statement::text(
        "INSERT INTO pickers_people (id, team_id, name, email) VALUES          (7, 1, 'Gauri Hundal', 'gauri@example.test'),          (8, 1, 'Arun Tiwari', 'arun@example.test'),          (9, 2, 'Anu Lakhotia', 'anu@example.test')",
    ))
    .await
    .unwrap();
    app
}

#[tokio::test]
async fn a_searched_list_answers_from_the_database_and_the_palette_gets_links() {
    let app = app().await;
    let list = app
        .client()
        .get("/people/options?q=gra")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(list.contains(r#"data-value="7""#), "{list}");
    assert!(list.contains("Gauri Hundal"), "{list}");
    assert!(!list.contains("Arun Tiwari"), "{list}");
    let palette = app
        .client()
        .get("/palette/search?q=arun")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(palette.contains("/people/8"), "{palette}");
}

#[tokio::test]
async fn a_post_naming_someone_who_is_not_there_is_refused_and_a_real_one_is_taken() {
    let app = app().await;
    let refused = app
        .client()
        .post("/picked")
        .header("content-type", FORM)
        .body("owner_id=999")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(
        refused.contains("Choose someone from the list."),
        "{refused}"
    );
    let taken = app
        .client()
        .post("/picked")
        .header("content-type", FORM)
        .body("owner_id=7")
        .send()
        .await
        .assert_status(200)
        .text();
    assert_eq!(taken, "saved");
}

#[tokio::test]
async fn a_refused_form_comes_back_with_the_name_of_the_person_picked() {
    let app = app().await;
    // The title is empty, so the form is refused; the person did pick Gauri.
    let page = app
        .client()
        .post("/deals")
        .header("content-type", FORM)
        .body("title=&owner_id=7")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(
        page.contains(r#"value="Gauri Hundal" placeholder="Search people""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<option value="7" selected>Gauri Hundal</option>"#),
        "{page}"
    );
    // Someone who is not there leaves the id, as before.
    let page = app
        .client()
        .post("/deals")
        .header("content-type", FORM)
        .body("title=&owner_id=999")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(
        page.contains(r#"value="999" placeholder="Search people""#),
        "{page}"
    );
}

#[tokio::test]
async fn a_refused_form_never_names_someone_outside_the_callers_scope() {
    let app = app().await;
    // Anu (9) is in another team. The post names her; the refusal shows what was posted, the
    // id, and no name: the label comes only from the caller's own scoped query.
    let page = app
        .client()
        .post("/deals")
        .header("content-type", FORM)
        .body("title=&owner_id=9")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(
        page.contains(r#"value="9" placeholder="Search people""#),
        "{page}"
    );
    assert!(!page.contains("Anu Lakhotia"), "{page}");
    assert!(!page.contains("anu@example.test"), "{page}");
    // Her name is not in the search either, and the check of what was picked refuses her.
    let list = app
        .client()
        .get("/people/options?q=anu")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(!list.contains("Anu Lakhotia"), "{list}");
    let refused = app
        .client()
        .post("/picked")
        .header("content-type", FORM)
        .body("owner_id=9")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(
        refused.contains("Choose someone from the list."),
        "{refused}"
    );
    // Someone in the team keeps their name, so the scope is the only difference.
    let page = app
        .client()
        .post("/deals")
        .header("content-type", FORM)
        .body("title=&owner_id=8")
        .send()
        .await
        .assert_status(422)
        .text();
    assert!(page.contains("Arun Tiwari"), "{page}");
}

// The report page of `components/date-range-picker.mdx` compiles as written, reads the
// window from the address, and refuses a window the limits do not allow in the person's
// own words.
#[cfg(feature = "time")]
#[tokio::test]
async fn the_report_window_example_reads_the_address_and_holds_it_to_the_limits() {
    let app = app().await;
    let html = app
        .client()
        .get("/report?from=2026-10-01&to=2026-10-31")
        .send()
        .await
        .text();
    assert!(
        html.contains("2026-10-01") && html.contains("2026-10-31"),
        "{html}"
    );
    // Ninety days at most: a year is refused with the picker's words.
    let response = app
        .client()
        .get("/report?from=2026-01-01&to=2026-12-31")
        .send()
        .await;
    assert_eq!(response.status(), http::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.text().contains("Choose at most 90 days."));
}
