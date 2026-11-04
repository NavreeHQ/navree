//! The routes: what each one loads and which view draws it.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | the board; `?assignee=`, `&label=`, `&due=` filter it |
//! | `GET /fragments/board` | the board's region, which the stream asks for again |
//! | `POST /board/moves` | a card's move, checked against the board as it is now |
//! | `GET /cards/{id}/peek`, `GET /cards/peek?card=` | the card in the drawer, as ops; its page with JavaScript off |
//! | `GET /cards/{id}` | the card on a page of its own |
//! | `POST /cards/{id}/move` | a move from the drawer's column field |
//! | `POST /cards/{id}/checks/moves` | a checklist line's move |
//! | `POST /checks/{id}/done` | a line ticked or not |
//! | `POST /cards/{id}/comments` | a comment |
//! | `POST /as/{id}` | shows the board as someone else: a lead, or a member who can't close cards |
//! | `GET /events` | the server-sent event stream |

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::NaiveDate;
use http::header::SET_COOKIE;
use http::{HeaderMap, Method, StatusCode};
use navree::db::{Db, Uow};
use navree::http::extract::{Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, RouteDef};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::ui::form::{FormView, Rejected};
use navree::ui::*;
use serde::Deserialize;

use crate::Settings;
use crate::data::{Check, Column, PROJECT, Person, TEAM, day_of};
use crate::live::{Hub, now};
use crate::store;
use crate::views::board::{DRAWER, Filters, REGION, board, board_page};
use crate::views::card::{
    CardData, CommentForm, card_detail, card_sheet, checklist, comments, missing_card,
};

/// The pages, with the hub that tells every open board when it changes.
#[injectable]
pub struct Pages {
    hub: Arc<Hub>,
    settings: Arc<Settings>,
    db: Arc<Db>,
}

type Result<T> = std::result::Result<T, AppError>;

/// The cookie that says who the board is shown as.
const AS: &str = "board_as";

/// Today, in UTC.
fn today() -> NaiveDate {
    day_of(now())
}

/// Who the board is shown as: the person the cookie names, or the lead.
fn viewer_id(headers: &HeaderMap) -> Option<i64> {
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == AS).then(|| value.parse().ok()).flatten()
        })
}

#[derive(Deserialize, Default)]
struct FilterQuery {
    assignee: Option<String>,
    label: Option<String>,
    due: Option<String>,
}

impl FilterQuery {
    fn filters(&self) -> Filters {
        Filters::parse(
            self.assignee.as_deref(),
            self.label.as_deref(),
            self.due.as_deref(),
        )
    }
}

#[derive(Deserialize, Default)]
struct PeekQuery {
    card: Option<String>,
}

impl Pages {
    async fn viewer(&self, uow: &Uow, headers: &HeaderMap) -> Result<Person> {
        Ok(store::viewer(uow, viewer_id(headers)).await?)
    }

    /// `GET /`.
    async fn index(
        &self,
        shell: AppShell,
        uow: Uow,
        headers: HeaderMap,
        query: FilterQuery,
    ) -> Result<AppPage> {
        let viewer = self.viewer(&uow, &headers).await?;
        let data = store::board(&uow).await?;
        Ok(board_page(
            shell.page(PROJECT).crumb(TEAM, "/"),
            &data,
            &viewer,
            &query.filters(),
            today(),
        ))
    }

    /// `GET /fragments/board`: the board's region alone.
    async fn fragment(&self, uow: Uow, headers: HeaderMap, query: FilterQuery) -> Result<View> {
        let viewer = self.viewer(&uow, &headers).await?;
        let data = store::board(&uow).await?;
        Ok(View::new("Board").body(board(&data, &viewer, &query.filters(), today())))
    }

    /// `POST /board/moves`: checks the move against the board as it is now, in the
    /// transaction that writes it. A refusal puts the card back; a stale page also
    /// fetches the board again.
    async fn moves(&self, headers: HeaderMap, mv: Move) -> Result<Reply> {
        let delay = self.hub.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let who = viewer_id(&headers);
        let checked = mv.clone();
        let outcome = self
            .db
            .transaction(|uow| async move {
                let viewer = store::viewer(&uow, who).await.map_err(store::db)?;
                let lists = store::columns(&uow, &viewer).await.map_err(store::db)?;
                match checked.check(&lists) {
                    Ok(landing) => {
                        if landing.moved {
                            store::move_card(&uow, &landing, viewer.id, now())
                                .await
                                .map_err(store::db)?;
                        }
                        Ok(Ok(landing))
                    }
                    Err(refusal) => Ok(Err(refusal)),
                }
            })
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        match outcome {
            Ok(landing) => {
                if landing.moved {
                    self.hub.nudge();
                }
                Ok(mv.done("/"))
            }
            Err(refusal) => {
                let reply = mv.refused("/", refusal.message());
                Ok(if refusal.is_stale() {
                    reply.op(Op::Invalidate {
                        target: REGION.to_owned(),
                    })
                } else {
                    reply
                })
            }
        }
    }

    /// `POST /cards/{id}/move`: the drawer's column field, a move to the end of a
    /// column, checked the same way. It answers with the drawer drawn again.
    async fn move_from_drawer(&self, headers: HeaderMap, id: i64, mv: Move) -> Result<Reply> {
        let to = Column::parse(&mv.to_list).map_or("that column", Column::title);
        let mv = Move::new(id.to_string(), mv.from_list, mv.to_list);
        let who = viewer_id(&headers);
        let checked = mv.clone();
        let outcome = self
            .db
            .transaction(|uow| async move {
                let viewer = store::viewer(&uow, who).await.map_err(store::db)?;
                let lists = store::columns(&uow, &viewer).await.map_err(store::db)?;
                let outcome = match checked.check(&lists) {
                    Ok(landing) if landing.from_list == landing.to_list => Ok(false),
                    Ok(landing) => {
                        store::move_card(&uow, &landing, viewer.id, now())
                            .await
                            .map_err(store::db)?;
                        Ok(true)
                    }
                    Err(refusal) => Err(refusal),
                };
                let card = store::card(&uow, id).await.map_err(store::db)?;
                Ok((outcome, card, viewer))
            })
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let (outcome, card, viewer) = outcome;
        let page = format!("/cards/{id}");
        let Some(card) = card else {
            return Ok(Reply::ops_or_redirect("/")
                .toast(ToastTone::Warning, "That card isn't on the board any more."));
        };
        let reply = match outcome {
            Ok(true) => {
                self.hub.nudge();
                mv.done(page)
                    .toast(ToastTone::Success, format!("Moved to {to}."))
            }
            Ok(false) => mv.done(page),
            Err(refusal) => mv.refused(page, refusal.message()),
        };
        Ok(reply.morph(DRAWER, card_sheet(&card, &viewer, today())))
    }

    /// The card in the drawer, opened.
    async fn peek(&self, uow: Uow, headers: HeaderMap, id: i64) -> Result<Reply> {
        let viewer = self.viewer(&uow, &headers).await?;
        let Some(card) = store::card(&uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/")
                .toast(ToastTone::Warning, "That card isn't on the board any more.")
                .morph(DRAWER, missing_card()));
        };
        Ok(Reply::ops_or_redirect(format!("/cards/{id}"))
            .morph(DRAWER, card_sheet(&card, &viewer, today()))
            .op(Op::Open {
                target: DRAWER.to_owned(),
            }))
    }

    /// `GET /cards/{id}`: the card on a page of its own.
    async fn card_page(
        &self,
        shell: AppShell,
        uow: Uow,
        headers: HeaderMap,
        id: i64,
    ) -> Result<Response> {
        let viewer = self.viewer(&uow, &headers).await?;
        let Some(card) = store::card(&uow, id).await? else {
            let view = shell
                .page("No such card")
                .crumb(PROJECT, "/")
                .body(
                    Empty::new("That card isn't on the board")
                        .icon(icons::SQUARE_KANBAN)
                        .description("Someone may have deleted it.")
                        .actions(Button::outline("Back to the board").href("/")),
                )
                .to_view()
                .status(StatusCode::NOT_FOUND);
            return Ok(view.into_response());
        };
        Ok(Self::card_view(&shell, &card, &viewer, None).into_response())
    }

    fn card_view(
        shell: &AppShell,
        card: &CardData,
        viewer: &Person,
        form: Option<FormView<CommentForm>>,
    ) -> AppPage {
        shell
            .page(card.card.title.clone())
            .crumb(PROJECT, "/")
            .body(Card::new().body(card_detail(card, viewer, today(), form)))
    }

    /// `POST /cards/{id}/checks/moves`: a checklist line moved.
    async fn move_check(&self, id: i64, mv: Move) -> Result<Reply> {
        let checked = mv.clone();
        let outcome = self
            .db
            .transaction(|uow| async move {
                let lines = store::checks(&uow, id).await.map_err(store::db)?;
                let list = ListState::new(
                    "checks",
                    "Checklist",
                    lines.iter().map(|line| line.id.to_string()),
                );
                match checked.check(&[list]) {
                    Ok(landing) => {
                        store::move_check(&uow, &landing).await.map_err(store::db)?;
                        Ok(Ok(landing.moved))
                    }
                    Err(refusal) => Ok(Err(refusal)),
                }
            })
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let page = format!("/cards/{id}");
        Ok(match outcome {
            Ok(moved) => {
                if moved {
                    self.hub.nudge();
                }
                mv.done(page)
            }
            Err(refusal) => mv.refused(page, refusal.message()),
        })
    }

    /// `POST /checks/{id}/done`: a line ticked, or not.
    async fn tick(&self, uow: Uow, line: i64, done: bool) -> Result<Reply> {
        let Some(found) = Check::query().find(&uow, line).await? else {
            return Ok(Reply::ops_or_redirect("/").toast(ToastTone::Warning, "That line is gone."));
        };
        let found = found.into_inner();
        Check::query()
            .where_(Check::ID.eq(line))
            .update(&uow, [Check::DONE.set(i64::from(done))])
            .await?;
        if done {
            store::record(
                &uow,
                found.card_id,
                1,
                now(),
                "checked",
                format!("Ticked \u{201c}{}\u{201d}", found.text),
            )
            .await?;
        }
        self.hub.nudge();
        let Some(card) = store::card(&uow, found.card_id).await? else {
            return Ok(Reply::ops_or_redirect("/"));
        };
        Ok(Reply::ops_or_redirect(format!("/cards/{}", found.card_id))
            .morph("checklist", checklist(&card)))
    }

    /// `POST /cards/{id}/comments`.
    async fn comment(
        &self,
        shell: AppShell,
        uow: Uow,
        headers: HeaderMap,
        id: i64,
        sent: std::result::Result<Submit<CommentForm>, Rejected<CommentForm>>,
    ) -> Result<Response> {
        let viewer = self.viewer(&uow, &headers).await?;
        let Some(card) = store::card(&uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/")
                .toast(ToastTone::Warning, "That card isn't on the board any more.")
                .into_response());
        };
        let comment = match sent {
            Ok(Submit(comment)) => comment,
            Err(rejected) => {
                return Ok(
                    rejected.respond(|form| Self::card_view(&shell, &card, &viewer, Some(form)))
                );
            }
        };
        store::comment(&uow, id, viewer.id, now(), comment.body.trim().to_owned()).await?;
        self.hub.nudge();
        let card = store::card(&uow, id).await?.unwrap_or(card);
        // Replaced, not morphed: a morph keeps what was typed, and the box should empty.
        Ok(Reply::ops_or_redirect(format!("/cards/{id}"))
            .op(Op::replace("comments", &comments(&card, None)))
            .toast(ToastTone::Success, "Comment added.")
            .into_response())
    }

    /// `POST /as/{id}`: shows the board as someone else.
    async fn view_as(&self, uow: Uow, id: i64) -> Result<Response> {
        let person = store::viewer(&uow, Some(id)).await?;
        let mut response = Reply::redirect("/")
            .toast(
                ToastTone::Info,
                format!(
                    "Viewing as {}, a {}.",
                    person.name,
                    if person.is_lead() { "lead" } else { "member" }
                ),
            )
            .into_response();
        if let Ok(cookie) = http::HeaderValue::from_str(&format!(
            "{AS}={}; Path=/; SameSite=Lax; HttpOnly",
            person.id
        )) {
            response.headers_mut().append(SET_COOKIE, cookie);
        }
        Ok(response)
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
                move |shell: AppShell,
                      uow: Uow,
                      headers: HeaderMap,
                      Query(query): Query<FilterQuery>| {
                    let me = me.clone();
                    async move { me.index(shell, uow, headers, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/board", {
                let me = me.clone();
                move |uow: Uow, headers: HeaderMap, Query(query): Query<FilterQuery>| {
                    let me = me.clone();
                    async move { me.fragment(uow, headers, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/board/moves", {
                let me = me.clone();
                move |headers: HeaderMap, mv: Move| {
                    let me = me.clone();
                    async move { me.moves(headers, mv).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/cards/peek", {
                let me = me.clone();
                move |uow: Uow, headers: HeaderMap, Query(query): Query<PeekQuery>| {
                    let me = me.clone();
                    async move {
                        let id = query
                            .card
                            .as_deref()
                            .and_then(|id| id.parse().ok())
                            .unwrap_or(0);
                        me.peek(uow, headers, id).await
                    }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/cards/{id}/peek", {
                let me = me.clone();
                move |uow: Uow, headers: HeaderMap, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.peek(uow, headers, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/cards/{id}", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, headers: HeaderMap, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.card_page(shell, uow, headers, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/cards/{id}/move", {
                let me = me.clone();
                move |headers: HeaderMap, Path(id): Path<i64>, mv: Move| {
                    let me = me.clone();
                    async move { me.move_from_drawer(headers, id, mv).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/cards/{id}/checks/moves", {
                let me = me.clone();
                move |Path(id): Path<i64>, mv: Move| {
                    let me = me.clone();
                    async move { me.move_check(id, mv).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/checks/{id}/done", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>, Submit(tick): Submit<TickForm>| {
                    let me = me.clone();
                    async move { me.tick(uow, id, tick.done.is_some()).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/cards/{id}/comments", {
                let me = me.clone();
                move |shell: AppShell,
                      uow: Uow,
                      headers: HeaderMap,
                      Path(id): Path<i64>,
                      sent: std::result::Result<Submit<CommentForm>, Rejected<CommentForm>>| {
                    let me = me.clone();
                    async move { me.comment(shell, uow, headers, id, sent).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/as/{id}", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.view_as(uow, id).await }
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

/// A line's tick, as a form: `done` is there when the box is ticked.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "tick")]
pub struct TickForm {
    /// Done
    #[validate(skip)]
    pub done: Option<String>,
}

#[derive(Deserialize, Default)]
struct MoveHook {
    item: Option<i64>,
    to: Option<String>,
    by: Option<i64>,
    quiet: Option<String>,
}

#[derive(Deserialize, Default)]
struct DelayHook {
    ms: Option<u64>,
}

/// What the browser tests drive the app with, when `BOARD_TEST_HOOKS` is set: the
/// board written afresh; a move made by someone else (`?item=4&to=ready&by=3`),
/// which with `&quiet=1` tells no open page, so the next move from one is stale;
/// and a wait before each move is answered (`?ms=3000`).
fn test_hooks(me: &Arc<Pages>) -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::POST, "!/__test/reset", {
            let me = me.clone();
            move || {
                let me = me.clone();
                async move {
                    me.hub.seed(&me.db, now()).await.map_err(|error| {
                        eprintln!("board-demo: reset failed: {error}");
                        AppError::internal(error.to_string())
                    })?;
                    me.hub.nudge();
                    Ok::<_, AppError>(StatusCode::NO_CONTENT)
                }
            }
        })
        .public(),
        RouteDef::on(Method::POST, "!/__test/move", {
            let me = me.clone();
            move |Query(hook): Query<MoveHook>| {
                let me = me.clone();
                async move {
                    let to = hook
                        .to
                        .as_deref()
                        .and_then(Column::parse)
                        .unwrap_or(Column::Ready);
                    let moved = me
                        .hub
                        .move_as(
                            &me.db,
                            hook.by.unwrap_or(1),
                            hook.item.unwrap_or(0),
                            to,
                            hook.quiet.is_none(),
                        )
                        .await
                        .map_err(|error| AppError::internal(error.to_string()))?;
                    Ok::<_, AppError>(if moved { "moved" } else { "refused" })
                }
            }
        })
        .public(),
        RouteDef::on(Method::POST, "!/__test/delay", {
            let me = me.clone();
            move |Query(hook): Query<DelayHook>| {
                let me = me.clone();
                async move {
                    me.hub
                        .delay_ms
                        .store(hook.ms.unwrap_or(0).min(60_000), Ordering::SeqCst);
                    Ok::<_, AppError>(StatusCode::NO_CONTENT)
                }
            }
        })
        .public(),
        RouteDef::on(Method::GET, "!/__test/board", {
            let me = me.clone();
            move || {
                let me = me.clone();
                async move {
                    let cards = me
                        .db
                        .transaction(
                            |uow| async move { store::cards(&uow).await.map_err(store::db) },
                        )
                        .await
                        .map_err(|error| AppError::internal(error.to_string()))?;
                    let text: Vec<String> = Column::ALL
                        .iter()
                        .map(|column| {
                            let ids: Vec<String> = cards
                                .iter()
                                .filter(|card| card.list == column.key())
                                .map(|card| card.id.to_string())
                                .collect();
                            format!("{}:{}", column.key(), ids.join(","))
                        })
                        .collect();
                    Ok::<_, AppError>(text.join(" "))
                }
            }
        })
        .public(),
    ]
}
