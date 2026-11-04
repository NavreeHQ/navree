//! The routes: what each one loads and which view draws it.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | the board; `?assignee=`, `&label=`, `&due=` filter it, `&show.<column>=` draws more of a column |
//! | `GET /fragments/board` | the board's region, which the stream asks for again |
//! | `GET /columns/{column}` | a column's next cards after a cursor, as ops; the board with more of it, with JavaScript off |
//! | `POST /board/moves` | a card's move, placed by rank and checked against the board as it is now |
//! | `GET /cards/search?q=` | the palette's answer: cards whose title holds the text |
//! | `GET /cards/{id}/peek` | the card in the drawer, as ops; its page with JavaScript off |
//! | `GET /cards/{id}` | the card on a page of its own |
//! | `GET /cards/{id}/comments`, `/history` | older comments or history after a cursor, as ops |
//! | `POST /cards/{id}/move` | a move from the drawer's column field |
//! | `POST /cards/{id}/checks/moves` | a checklist line's move |
//! | `POST /checks/{id}/done` | a line ticked or not |
//! | `POST /cards/{id}/comments` | a comment |
//! | `POST /as/{id}` | shows the board as someone else: a lead, or a member who can't close cards |
//! | `GET /events` | the server-sent event stream |

use std::collections::HashMap;
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
use crate::data::{Check, Column, ColumnRow, PROJECT, Person, TEAM, day_of};
use crate::live::{Hub, now};
use crate::store::{self, parse_cursor};
use crate::views::board::{
    ColumnPage, DRAWER, Filters, PAGE, REGION, board, board_page, column, query,
};
use crate::views::card::{
    CardData, CommentForm, card_detail, card_sheet, checklist, comment_items, comments,
    history_page, missing_card, older_comments, older_history,
};

/// The pages, with the hub that tells every open board when it changes.
#[injectable]
pub struct Pages {
    hub: Arc<Hub>,
    settings: Arc<Settings>,
    db: Arc<Db>,
}

type Result<T> = std::result::Result<T, AppError>;
type Params = HashMap<String, String>;

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

fn filters(params: &Params) -> Filters {
    Filters::parse(
        params.get("assignee").map(String::as_str),
        params.get("label").map(String::as_str),
        params.get("due").map(String::as_str),
    )
}

/// What a transaction's failure answers.
fn failed(error: impl std::fmt::Display) -> AppError {
    AppError::internal(error.to_string())
}

#[derive(Deserialize, Default)]
struct Before {
    before: Option<String>,
}

#[derive(Deserialize, Default)]
struct Search {
    q: Option<String>,
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
        params: Params,
    ) -> Result<AppPage> {
        let viewer = self.viewer(&uow, &headers).await?;
        let filters = filters(&params);
        let data = store::board(&uow, &filters, today(), |name| params.get(name).cloned()).await?;
        Ok(board_page(
            shell.page(PROJECT).crumb(TEAM, "/"),
            &data,
            &viewer,
            &filters,
            today(),
        ))
    }

    /// `GET /fragments/board`: the board's region alone.
    async fn fragment(&self, uow: Uow, headers: HeaderMap, params: Params) -> Result<View> {
        let viewer = self.viewer(&uow, &headers).await?;
        let filters = filters(&params);
        let data = store::board(&uow, &filters, today(), |name| params.get(name).cloned()).await?;
        Ok(View::new("Board").body(board(&data, &viewer, &filters, today())))
    }

    /// `GET /columns/{column}?after=&show=`: the next cards of a column, joined to its
    /// end in the page. With JavaScript off, the board with that much of the column.
    async fn more(
        &self,
        uow: Uow,
        headers: HeaderMap,
        key: String,
        params: Params,
    ) -> Result<Reply> {
        let Some(which) = Column::parse(&key) else {
            return Ok(Reply::ops_or_redirect("/"));
        };
        let filters = filters(&params);
        let shown = store::shown(which, |_| params.get("show").cloned());
        let mut back = filters.pairs();
        back.push((format!("show.{key}"), shown.to_string()));
        let reply = Reply::ops_or_redirect(format!("/{}", query(&back)));
        let Some(after) = params.get("after").and_then(|text| parse_cursor(text)) else {
            return Ok(reply);
        };
        let viewer = self.viewer(&uow, &headers).await?;
        let start = shown.saturating_sub(PAGE);
        let (cards, more) =
            store::column_cards(&uow, which, &filters, today(), Some(after), PAGE).await?;
        let page = ColumnPage {
            next: more.then(|| cards.last().map(store::cursor)).flatten(),
            total: store::column_total(&uow, which).await?,
            column: which,
            cards,
            start,
        };
        let people = store::people(&uow).await?;
        // Every column of the board is named, so a moved card's form offers them all.
        let lists = Column::ALL.map(|other| {
            if other == which {
                column(&page, &people, &viewer, &filters, today())
            } else {
                SortList::new(other.key(), other.title())
            }
        });
        let ops = Sortable::new("columns", format!("{PROJECT} board"))
            .board()
            .post("/board/moves")
            .lists(lists)
            .page(which.key());
        Ok(ops.into_iter().fold(reply, Reply::op))
    }

    /// `POST /board/moves`: places the move by rank, checked against the board as it
    /// is now, in the transaction that writes it. A refusal puts the card back; a
    /// stale page also fetches the board again.
    async fn moves(&self, headers: HeaderMap, mv: Move) -> Result<Reply> {
        let delay = self.hub.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        let who = viewer_id(&headers);
        let made = mv.clone();
        let placed = self
            .db
            .transaction(|uow| async move {
                // The columns are locked before anything is read, so every read sees
                // the moves that went first, whatever the database's isolation.
                made.lock(&uow, &ColumnRow::KEY).await.map_err(store::db)?;
                let viewer = store::viewer(&uow, who).await.map_err(store::db)?;
                store::place_and_write(&uow, &viewer, &made, now())
                    .await
                    .map_err(store::db)
            })
            .await
            .map_err(failed)?;
        match placed {
            Ok(placement) => {
                if placement.moved {
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
    /// column, placed the same way. It answers with the drawer drawn again.
    async fn move_from_drawer(&self, headers: HeaderMap, id: i64, mv: Move) -> Result<Reply> {
        let to = Column::parse(&mv.to_list).map_or("that column", Column::title);
        let mv = Move::new(id.to_string(), mv.from_list, mv.to_list);
        let who = viewer_id(&headers);
        let made = mv.clone();
        let (placed, card, viewer) = self
            .db
            .transaction(|uow| async move {
                made.lock(&uow, &ColumnRow::KEY).await.map_err(store::db)?;
                let viewer = store::viewer(&uow, who).await.map_err(store::db)?;
                let placed = if made.from_list == made.to_list {
                    None
                } else {
                    Some(
                        store::place_and_write(&uow, &viewer, &made, now())
                            .await
                            .map_err(store::db)?,
                    )
                };
                let card = store::card(&uow, id).await.map_err(store::db)?;
                Ok((placed, card, viewer))
            })
            .await
            .map_err(failed)?;
        let page = format!("/cards/{id}");
        let Some(card) = card else {
            return Ok(Reply::ops_or_redirect("/")
                .toast(ToastTone::Warning, "That card isn't on the board any more."));
        };
        let reply = match placed {
            Some(Ok(_)) => {
                self.hub.nudge();
                mv.done(page)
                    .toast(ToastTone::Success, format!("Moved to {to}."))
            }
            Some(Err(refusal)) => mv.refused(page, refusal.message()),
            None => mv.done(page),
        };
        Ok(reply.morph(DRAWER, card_sheet(&card, &viewer, today())))
    }

    /// `GET /cards/search?q=`: the cards whose title holds the text, for the palette.
    async fn search(&self, uow: Uow, text: &str) -> Result<ChoiceList> {
        let cards = store::search(&uow, text).await?;
        let found: Vec<Choice> = cards
            .into_iter()
            .map(|card| {
                let column = Column::parse(&card.list).map_or("", Column::title);
                Choice::new(card.id.to_string(), card.title)
                    .description(column)
                    .link(format!("/cards/{}", card.id))
            })
            .collect();
        Ok(ChoiceList::new().group("Cards", found))
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
            .body(Card::titled("About this card").heading(2).body(card_detail(
                card,
                viewer,
                today(),
                form,
            )))
    }

    /// `GET /cards/{id}/comments?before=`: older comments, joined to the list.
    async fn older_comments(&self, uow: Uow, id: i64, before: Option<String>) -> Result<Reply> {
        let reply = Reply::ops_or_redirect(format!("/cards/{id}"));
        let Some(before) = before.as_deref().and_then(parse_cursor) else {
            return Ok(reply);
        };
        let (page, next) = store::comments(&uow, id, Some(before)).await?;
        let people = store::people(&uow).await?;
        Ok(reply
            .op(Op::append(
                "comment-list",
                &Node::list(comment_items(&page, &people)),
            ))
            .op(Op::replace(
                "comments-more",
                &older_comments(id, next.as_deref()),
            )))
    }

    /// `GET /cards/{id}/history?before=`: older lines of history.
    async fn older_history(&self, uow: Uow, id: i64, before: Option<String>) -> Result<Reply> {
        let reply = Reply::ops_or_redirect(format!("/cards/{id}"));
        let Some(cursor) = before.as_deref().and_then(parse_cursor) else {
            return Ok(reply);
        };
        let (page, next) = store::history(&uow, id, Some(cursor)).await?;
        let people = store::people(&uow).await?;
        let name = before.unwrap_or_default().replace('.', "-");
        Ok(reply
            .op(Op::append(
                "history-list",
                &history_page(&page, &people, &name),
            ))
            .op(Op::replace(
                "history-more",
                &older_history(id, next.as_deref()),
            )))
    }

    /// `POST /cards/{id}/checks/moves`: a checklist line moved. A card's lines are
    /// few, so the whole list is read and checked, under a lock on the card's row:
    /// its checklist is that card's.
    async fn move_check(&self, id: i64, mv: Move) -> Result<Reply> {
        let made = mv.clone();
        let outcome = self
            .db
            .transaction(|uow| async move {
                navree::ui::sortable::lock_lists(&uow, &crate::data::Card::ID, [id])
                    .await
                    .map_err(store::db)?;
                let lines = store::checks(&uow, id).await.map_err(store::db)?;
                let list = ListState::new(
                    "checks",
                    "Checklist",
                    lines.iter().map(|line| line.id.to_string()),
                );
                match made.check(&[list]) {
                    Ok(landing) => {
                        store::move_check(&uow, &landing.order)
                            .await
                            .map_err(store::db)?;
                        Ok(Ok(landing.moved))
                    }
                    Err(refusal) => Ok(Err(refusal)),
                }
            })
            .await
            .map_err(failed)?;
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
    async fn tick(&self, uow: Uow, headers: HeaderMap, line: i64, done: bool) -> Result<Reply> {
        let Some(found) = Check::query().find(&uow, line).await? else {
            return Ok(Reply::ops_or_redirect("/").toast(ToastTone::Warning, "That line is gone."));
        };
        let found = found.into_inner();
        let viewer = self.viewer(&uow, &headers).await?;
        store::tick(&uow, &found, done).await?;
        if done {
            store::record(
                &uow,
                found.card_id,
                viewer.id,
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
        let role = if person.is_lead() { "lead" } else { "member" };
        let mut response = Reply::redirect("/")
            .toast(
                ToastTone::Info,
                format!("Viewing as {}, a {role}.", person.name),
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
                      Query(params): Query<Params>| {
                    let me = me.clone();
                    async move { me.index(shell, uow, headers, params).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/board", {
                let me = me.clone();
                move |uow: Uow, headers: HeaderMap, Query(params): Query<Params>| {
                    let me = me.clone();
                    async move { me.fragment(uow, headers, params).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/columns/{column}", {
                let me = me.clone();
                move |uow: Uow,
                      headers: HeaderMap,
                      Path(key): Path<String>,
                      Query(params): Query<Params>| {
                    let me = me.clone();
                    async move { me.more(uow, headers, key, params).await }
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
            RouteDef::on(Method::GET, "!/cards/search", {
                let me = me.clone();
                move |uow: Uow, Query(search): Query<Search>| {
                    let me = me.clone();
                    async move {
                        me.search(uow, search.q.as_deref().unwrap_or_default())
                            .await
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
            RouteDef::on(Method::GET, "!/cards/{id}/comments", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>, Query(page): Query<Before>| {
                    let me = me.clone();
                    async move { me.older_comments(uow, id, page.before).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/cards/{id}/history", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>, Query(page): Query<Before>| {
                    let me = me.clone();
                    async move { me.older_history(uow, id, page.before).await }
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
                move |uow: Uow,
                      headers: HeaderMap,
                      Path(id): Path<i64>,
                      Submit(tick): Submit<TickForm>| {
                    let me = me.clone();
                    async move { me.tick(uow, headers, id, tick.done.is_some()).await }
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

#[derive(Deserialize, Default)]
struct WhereHook {
    ids: Option<String>,
}

/// What the browser tests drive the app with, when `BOARD_TEST_HOOKS` is set: the
/// board written afresh; a move made by someone else (`?item=4&to=ready&by=3`),
/// which with `&quiet=1` tells no open page, so the next move from one is stale; a
/// wait before each move is answered (`?ms=3000`); and where cards are
/// (`?ids=1,2`: `1:backlog:0 2:ready:3`, each with its place in its column).
fn test_hooks(me: &Arc<Pages>) -> Vec<RouteDef> {
    use crate::data::Card as Stored;
    vec![
        RouteDef::on(Method::POST, "!/__test/reset", {
            let me = me.clone();
            move || {
                let me = me.clone();
                async move {
                    me.hub.seed(&me.db, now()).await.map_err(failed)?;
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
                        .map_err(failed)?;
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
        RouteDef::on(
            Method::GET,
            "!/__test/where",
            |uow: Uow, Query(hook): Query<WhereHook>| async move {
                let mut out = Vec::new();
                for id in hook
                    .ids
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|id| id.parse::<i64>().ok())
                {
                    let Some(card) = Stored::query().find(&uow, id).await? else {
                        out.push(format!("{id}:gone:0"));
                        continue;
                    };
                    let card = card.into_inner();
                    let before = Stored::query()
                        .where_(Stored::LIST.eq(card.list.clone()))
                        .where_(
                            Stored::RANK
                                .lt(card.rank)
                                .or(Stored::RANK.eq(card.rank).and(Stored::ID.lt(id))),
                        )
                        .count(&uow)
                        .await?;
                    out.push(format!("{id}:{}:{before}", card.list));
                }
                Ok::<_, AppError>(out.join(" "))
            },
        )
        .public(),
    ]
}
