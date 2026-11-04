//! The routes: what each page loads and which view draws it.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | the overview: figures, chart, funnel and the live feed (`?period=1h\|24h\|all`) |
//! | `GET /recipients` | the recipients table; sort, filters, search and page in the URL |
//! | `GET /recipients/{id}` | one recipient, as a page |
//! | `GET /recipients/{id}/peek` | the same in the sheet, as ops; a redirect to the page with JavaScript off |
//! | `GET`, `POST /recipients/{id}/edit` | the contact form |
//! | `GET /fragments/...` | what the live parts fetch when the stream speaks |
//! | `GET /events` | the server-sent event stream |
//! | `GET /flags/{code}.svg` | a country's flag as a picture, for the lists of countries |

use std::sync::Arc;

use chrono::NaiveDate;
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::request::Parts;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use navree::db::{Db, Uow};
use navree::http::extract::{Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, RouteDef};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::ui::flags::CountryCode;
use navree::ui::form::{FieldErrors, FormView, Rejected};
use navree::ui::*;
use serde::Deserialize;

use crate::Settings;
use crate::data::{CAMPAIGN, Event, Kind, OFFSET, Recipient, SUBJECT, Status, day_of};
use crate::live::{Hub, now};
use crate::views::contact::{ContactForm, contact_form, countries};
use crate::views::dashboard::{
    Counts, FeedEntry, Period, activity_feed, activity_item, engagement, figures, hourly,
    live_engagement, live_figures,
};
use crate::views::recipient::{recipient_detail, recipient_sheet};
use crate::views::recipients::{RecipientRow, SHEET, empty_sheet, recipients_table};
use crate::views::{moment, thousands};

/// The pages, with the hub that knows when the campaign moves.
#[injectable]
pub struct Pages {
    hub: Arc<Hub>,
    settings: Arc<Settings>,
    db: Arc<Db>,
}

type Result<T> = std::result::Result<T, AppError>;

/// Today in the campaign's zone.
fn today() -> NaiveDate {
    day_of(now()).unwrap_or_default()
}

/// How far the campaign has got, counted in the database.
async fn counts(uow: &Uow) -> Result<Counts> {
    let with = |status: Status| Recipient::query().where_(Recipient::STATUS.eq(status.key()));
    let sent = Recipient::query().count(uow).await?;
    let bounced = with(Status::Bounced).count(uow).await?;
    Ok(Counts {
        sent,
        delivered: sent - bounced,
        opened: Recipient::query()
            .where_(Recipient::OPENS.gt(0))
            .count(uow)
            .await?,
        clicked: Recipient::query()
            .where_(Recipient::CLICKS.gt(0))
            .count(uow)
            .await?,
        converted: with(Status::Converted).count(uow).await?,
        unsubscribed: with(Status::Unsubscribed).count(uow).await?,
        bounced,
        proxied: Event::query()
            .where_(Event::KIND.eq(Kind::ProxyOpen.key()))
            .count(uow)
            .await?,
    })
}

/// How many recipients hold each status.
async fn status_counts(uow: &Uow) -> Result<Vec<(Status, u64)>> {
    let mut out = Vec::with_capacity(Status::ALL.len());
    for status in Status::ALL {
        let n = Recipient::query()
            .where_(Recipient::STATUS.eq(status.key()))
            .count(uow)
            .await?;
        out.push((status, n));
    }
    Ok(out)
}

/// Opens and clicks by hour since the campaign was sent.
async fn hours(uow: &Uow, sent_at: i64) -> Result<Vec<(u32, u32)>> {
    let events: Vec<Event> = Event::query()
        .where_(Event::KIND.is_in([Kind::Opened.key(), Kind::Clicked.key()]))
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    Ok(hourly(&events, sent_at, now()))
}

/// What the feed shows: what readers and proxies did in the period, newest
/// first, after `after` when given; and how many there are in all.
async fn feed(
    uow: &Uow,
    period: Period,
    after: Option<i64>,
    limit: u64,
) -> Result<(Vec<FeedEntry>, u64)> {
    let kinds = [
        Kind::Opened,
        Kind::ProxyOpen,
        Kind::Clicked,
        Kind::Converted,
        Kind::Unsubscribed,
        Kind::Bounced,
    ]
    .map(Kind::key);
    let query = Event::query()
        .where_(Event::KIND.is_in(kinds))
        .where_(Event::AT.gte(period.since(now())))
        .where_(Event::ID.gt(after.unwrap_or(0)));
    let total = query.clone().count(uow).await?;
    let events: Vec<Event> = query
        .order_by_desc("id")
        .limit(limit)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let ids: Vec<i64> = events.iter().map(|event| event.recipient_id).collect();
    let people: Vec<Recipient> = Recipient::query()
        .find_many(uow, ids)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let entries = events
        .into_iter()
        .map(|event| FeedEntry {
            name: people
                .iter()
                .find(|person| person.id == event.recipient_id)
                .map_or_else(|| "Someone".to_owned(), |person| person.name.clone()),
            event,
        })
        .collect();
    Ok((entries, total))
}

/// A recipient and the events of their copy, oldest first.
async fn recipient(uow: &Uow, id: i64) -> Result<Option<(Recipient, Vec<Event>)>> {
    let Some(person) = Recipient::query().find(uow, id).await? else {
        return Ok(None);
    };
    let events = Event::query()
        .where_(Event::RECIPIENT_ID.eq(id))
        .order_by("at")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    Ok(Some((person.into_inner(), events)))
}

/// A recipient who isn't on the list.
fn missing(shell: &AppShell) -> Response {
    let view = shell
        .page("No such recipient")
        .crumb("Recipients", "/recipients")
        .body(
            Empty::new("That recipient isn't on the list")
                .icon(icons::USER_X)
                .description("They may have been removed. Search the list for them by name.")
                .actions(Button::outline("Back to recipients").href("/recipients")),
        )
        .to_view()
        .status(StatusCode::NOT_FOUND);
    view.into_response()
}

/// An HTML fragment for a region.
fn fragment(title: &'static str, body: impl IntoNodes) -> View {
    View::new(title).body(body)
}

#[derive(Deserialize, Default)]
struct PeriodQuery {
    period: Option<String>,
    after: Option<i64>,
}

#[derive(Deserialize, Default)]
struct StepQuery {
    kind: Option<String>,
    recipient: Option<i64>,
}

impl Pages {
    /// `GET /`.
    async fn overview(&self, shell: AppShell, uow: Uow, period: Period) -> Result<AppPage> {
        let sent_at = self.hub.sent_at();
        let counts = counts(&uow).await?;
        let hours = hours(&uow, sent_at).await?;
        let (entries, total) = feed(&uow, period, None, 80).await?;
        Ok(shell
            .page(CAMPAIGN)
            .actions(
                Button::outline("Recipients")
                    .icon(icons::USERS)
                    .href("/recipients"),
            )
            .body(Stack::gap(4).children((
                Text::muted(format!(
                    "Sent to {} people on {}. Subject: “{SUBJECT}”",
                    thousands(counts.sent),
                    moment(sent_at)
                )),
                live_figures(&counts),
                Grid::cols(2).gap(4).children((
                    activity_feed(&entries, period, total),
                    live_engagement(&counts, &hours, sent_at),
                )),
            ))))
    }

    /// `GET /fragments/activity?after=`: only the rows newer than the cursor, as
    /// HTML, with the newest cursor in `X-UI-Cursor`.
    async fn activity(&self, uow: Uow, query: PeriodQuery) -> Result<Response> {
        let period = Period::parse(query.period.as_deref());
        let (entries, _) = feed(&uow, period, query.after, 100).await?;
        let html = render(&ActivityFeed::rows(
            entries.iter().map(activity_item),
            OFFSET,
        ));
        let mut response = (
            [
                (CONTENT_TYPE, "text/html; charset=utf-8"),
                (CACHE_CONTROL, "no-store"),
            ],
            html,
        )
            .into_response();
        if let Some(newest) = entries.first()
            && let Ok(value) = HeaderValue::from_str(&newest.event.id.to_string())
        {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-ui-cursor"), value);
        }
        Ok(response)
    }

    /// `GET /recipients`.
    async fn recipients(
        &self,
        shell: AppShell,
        uow: Uow,
        query: TableQuery<RecipientRow>,
    ) -> Result<AppPage> {
        let page = query
            .load_as(Recipient::query(), &uow, |record| {
                RecipientRow::from(record.into_inner())
            })
            .await?;
        let statuses = status_counts(&uow).await?;
        Ok(shell.page("Recipients").crumb(CAMPAIGN, "/").body((
            recipients_table(&query, &page, &statuses, today()),
            empty_sheet(),
        )))
    }

    /// `GET /recipients/{id}`.
    async fn show(&self, shell: AppShell, uow: Uow, id: i64) -> Result<Response> {
        let Some((person, events)) = recipient(&uow, id).await? else {
            return Ok(missing(&shell));
        };
        Ok(shell
            .page(person.name.clone())
            .crumb(CAMPAIGN, "/")
            .crumb("Recipients", "/recipients")
            .actions(
                Button::primary("Edit contact")
                    .icon(icons::PENCIL)
                    .href(format!("/recipients/{id}/edit")),
            )
            .body(recipient_detail(&person, &events, 2))
            .into_response())
    }

    /// `GET /recipients/{id}/peek`: the recipient in the sheet, opened.
    async fn peek(&self, uow: Uow, id: i64) -> Result<Reply> {
        let Some((person, events)) = recipient(&uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/recipients").toast(
                ToastTone::Warning,
                "That recipient isn't on the list any more.",
            ));
        };
        Ok(Reply::ops_or_redirect(format!("/recipients/{id}"))
            .morph(SHEET, recipient_sheet(&person, &events))
            .op(Op::Open {
                target: SHEET.to_owned(),
            }))
    }

    fn edit_page(shell: &AppShell, person: &Recipient, form: FormView<ContactForm>) -> AppPage {
        shell
            .page(format!("Edit {}", person.name))
            .crumb("Recipients", "/recipients")
            .crumb(person.name.clone(), format!("/recipients/{}", person.id))
            .body(contact_form(form, person.id, today()))
    }

    /// `GET /recipients/{id}/edit`.
    async fn edit(&self, shell: AppShell, uow: Uow, id: i64) -> Result<Response> {
        let Some((person, _)) = recipient(&uow, id).await? else {
            return Ok(missing(&shell));
        };
        let form = FormView::new().of(&ContactForm::of(&person));
        Ok(Self::edit_page(&shell, &person, form).into_response())
    }

    /// `POST /recipients/{id}/edit`: saves, or shows what to fix under the fields.
    async fn update(
        &self,
        shell: AppShell,
        uow: Uow,
        parts: Parts,
        id: i64,
        sent: std::result::Result<Submit<ContactForm>, Rejected<ContactForm>>,
    ) -> Result<Response> {
        let Some((person, _)) = recipient(&uow, id).await? else {
            return Ok(missing(&shell));
        };
        let contact = match sent {
            Ok(Submit(contact)) => contact,
            Err(rejected) => {
                return Ok(rejected.respond(|form| Self::edit_page(&shell, &person, form)));
            }
        };
        if !Choice::all_among([contact.country.as_str()], &countries()) {
            let errors = FieldErrors::from([(
                "country".to_owned(),
                vec!["Choose a country from the list.".to_owned()],
            )]);
            return Ok(Rejected::after_decode(&contact, errors, &parts)
                .respond(|form| Self::edit_page(&shell, &person, form)));
        }
        let city = if contact.country == person.country {
            person.city.clone()
        } else {
            String::new()
        };
        Recipient::query()
            .where_(Recipient::ID.eq(id))
            .update(
                &uow,
                [
                    Recipient::NAME.set(contact.name.trim().to_owned()),
                    Recipient::EMAIL.set(contact.email.trim().to_owned()),
                    Recipient::ROLE.set(contact.role.trim().to_owned()),
                    Recipient::PHONE.set(
                        contact
                            .mobile
                            .as_ref()
                            .map_or_else(String::new, |phone| phone.as_str().to_owned()),
                    ),
                    Recipient::COUNTRY.set(contact.country.clone()),
                    Recipient::CITY.set(city),
                    Recipient::CUSTOMER_SINCE.set(contact.customer_since),
                ],
            )
            .await?;
        self.hub.nudge();
        Ok(Reply::redirect(format!("/recipients/{id}"))
            .toast_with(
                Toast::new(
                    ToastTone::Success,
                    format!("{}'s details are saved.", contact.name.trim()),
                )
                .title("Contact saved"),
            )
            .into_response())
    }

    /// `GET /flags/{code}.svg`: a country's flag as a picture, cached for good.
    fn flag(file: &str) -> Response {
        let code = file.strip_suffix(".svg").unwrap_or(file);
        let Some(country) = CountryCode::parse(code) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 512 512\">{}</svg>",
            country.square_shapes()
        );
        (
            [
                (CONTENT_TYPE, "image/svg+xml"),
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            svg,
        )
            .into_response()
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let me = self.clone();
        let mut routes = vec![
            RouteDef::on(Method::GET, "!/", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, Query(query): Query<PeriodQuery>| {
                    let me = me.clone();
                    async move {
                        me.overview(shell, uow, Period::parse(query.period.as_deref()))
                            .await
                    }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/activity", {
                let me = me.clone();
                move |uow: Uow, Query(query): Query<PeriodQuery>| {
                    let me = me.clone();
                    async move { me.activity(uow, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/figures", |uow: Uow| async move {
                Ok::<_, AppError>(fragment("Figures", figures(&counts(&uow).await?)))
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/engagement", {
                let me = me.clone();
                move |uow: Uow| {
                    let me = me.clone();
                    async move {
                        let sent_at = me.hub.sent_at();
                        let counts = counts(&uow).await?;
                        let hours = hours(&uow, sent_at).await?;
                        Ok::<_, AppError>(fragment(
                            "Engagement",
                            engagement(&counts, &hours, sent_at),
                        ))
                    }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/events", {
                let me = me.clone();
                move |headers: HeaderMap| {
                    let me = me.clone();
                    async move { me.hub.stream(&headers) }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/recipients", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, query: TableQuery<RecipientRow>| {
                    let me = me.clone();
                    async move { me.recipients(shell, uow, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/recipients/{id}", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.show(shell, uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/recipients/{id}/peek", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.peek(uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/recipients/{id}/edit", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.edit(shell, uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/recipients/{id}/edit", {
                let me = me.clone();
                move |shell: AppShell,
                      uow: Uow,
                      parts: Parts,
                      Path(id): Path<i64>,
                      sent: std::result::Result<Submit<ContactForm>, Rejected<ContactForm>>| {
                    let me = me.clone();
                    async move { me.update(shell, uow, parts, id, sent).await }
                }
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/flags/{file}",
                |Path(file): Path<String>| async move { Self::flag(&file) },
            )
            .public(),
            RouteDef::on(Method::POST, "!/sign-out", || async {
                Reply::redirect("/")
                    .toast(ToastTone::Info, "This demo has no accounts to sign out of.")
            })
            .public(),
        ];
        if self.settings.test_hooks {
            routes.extend(test_hooks(&me));
        }
        routes
    }
}

/// What the browser tests drive the app with, when `CAMPAIGN_TEST_HOOKS` is set:
/// one step of a kind they choose (`?kind=clicked`, `&recipient=7` for who), and the
/// campaign written afresh.
fn test_hooks(me: &Arc<Pages>) -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::POST, "!/__test/step", {
            let me = me.clone();
            move |Query(query): Query<StepQuery>| {
                let me = me.clone();
                async move {
                    let kind = query.kind.as_deref().map(Kind::parse);
                    let event = me
                        .hub
                        .step(&me.db, now(), kind, query.recipient)
                        .await
                        .map_err(|error| AppError::internal(error.to_string()))?;
                    Ok::<_, AppError>(event.map_or_else(String::new, |event| {
                        format!("{} {}", event.id, event.recipient_id)
                    }))
                }
            }
        })
        .public(),
        RouteDef::on(Method::POST, "!/__test/reset", {
            let me = me.clone();
            move || {
                let me = me.clone();
                async move {
                    me.hub.seed(&me.db, now()).await.map_err(|error| {
                        eprintln!("campaign-demo: reset failed: {error}");
                        AppError::internal(error.to_string())
                    })?;
                    me.hub.nudge();
                    Ok::<_, AppError>(StatusCode::NO_CONTENT)
                }
            }
        })
        .public(),
    ]
}
