//! The routes: what each page loads and which view draws it.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | the dashboard: figures, the last 30 days of sales, the live order activity, low stock, top sellers |
//! | `GET /orders` | the orders table; sort, filters, search and page in the URL |
//! | `GET /orders/{id}` | one order, as a page |
//! | `GET /orders/{id}/peek` | the same in the drawer, as ops; a redirect to the page with JavaScript off |
//! | `POST /orders/{id}/refund`, `/cancel`, `/resend` | the actions, each checked on the server |
//! | `GET /inventory` | the products, with a restock form in each row |
//! | `GET /inventory/{id}/peek` | the product in a sheet, as ops, for a screen drawn by the system's own toolkit |
//! | `POST /inventory/{id}/restock` | adds units to a product |
//! | `GET /fragments/...` | what the live parts fetch when the stream speaks |
//! | `GET /broadcasting/stream` | the live hub's stream (`RealtimeModule`): the topics `orders`, `activity`, `stats` and `inventory` |

use std::sync::Arc;

use chrono::NaiveDate;
use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use navree::db::{Db, Uow};
use navree::http::extract::{Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, RouteDef};
use navree::orm::Model as _;
use navree::prelude::*;
use navree::ui::charts::{Metric, Report, TimeColumn};
use navree::ui::form::Rejected;
use navree::ui::*;
use serde::Deserialize;

use crate::Settings;
use crate::data::{
    Customer, Event, Fulfilment, Kind, Level, Line, OFFSET, Order, Product, Status, day_of,
};
use crate::live::{Hub, now};
use crate::shop::{self, Done, Refusal};
use crate::views::dashboard::{
    Cursor, FeedEntry, Figures, METHODS, Sales, Seller, activity_item, dashboard_body, figures,
    low_stock, sales_charts, sales_report, shelves, top_sellers,
};
use crate::views::inventory::{
    self, ProductRow, RestockForm, empty_sheet, inventory_table, product_sheet,
};
use crate::views::order::{
    ConfirmForm, OrderView, PAGE, Parts, RefundForm, order_drawer, order_page, other_orders,
    refund_dialog, steps,
};
use crate::views::orders::{DRAWER, OrderRow, empty_drawer, orders_table};
use crate::views::units;

/// The pages, with the hub that knows when the shop moves.
#[injectable]
pub struct Pages {
    hub: Arc<Hub>,
    settings: Arc<Settings>,
    db: Arc<Db>,
}

type Result<T> = std::result::Result<T, AppError>;

/// Today in the shop's zone.
fn today() -> NaiveDate {
    day_of(now())
}

fn inner<M>(records: Vec<navree::orm::Record<M>>) -> Vec<M>
where
    M: navree::orm::Model,
{
    records
        .into_iter()
        .map(|record| record.into_inner())
        .collect()
}

/// Today's figures, counted in the database.
async fn count_figures(uow: &Uow) -> Result<Figures> {
    let now = now();
    let today: Vec<Order> = inner(
        Order::query()
            .where_(Order::PLACED_ON.eq(day_of(now)))
            .get(uow)
            .await?
            .into_vec(),
    );
    let open = |step: Fulfilment| {
        Order::query()
            .where_(Order::STATUS.eq(Status::Open.key()))
            .where_(Order::FULFILMENT.eq(step.key()))
    };
    let level = |level: Level| Product::query().where_(Product::LEVEL.eq(level.key()));
    Ok(Figures {
        revenue: today
            .iter()
            .filter(|order| order.status != Status::Cancelled.key())
            .map(|order| order.total - order.refunded)
            .sum(),
        orders: i64::try_from(today.len()).unwrap_or(0),
        last_hour: i64::try_from(
            today
                .iter()
                .filter(|order| order.placed_at >= now - 3600)
                .count(),
        )
        .unwrap_or(0),
        to_pack: i64::try_from(open(Fulfilment::Unfulfilled).count(uow).await?).unwrap_or(0),
        to_ship: i64::try_from(open(Fulfilment::Packed).count(uow).await?).unwrap_or(0),
        low: i64::try_from(level(Level::Low).count(uow).await?).unwrap_or(0),
        out: i64::try_from(level(Level::Out).count(uow).await?).unwrap_or(0),
    })
}

/// The last 30 days of sales, counted in the database: three grouped queries over
/// the days' orders, each read through the index on `placed_at`.
async fn sales(uow: &Uow, report: &Report) -> Result<Sales> {
    let placed = TimeColumn::UnixSeconds("placed_at");
    // Orders a day by how they are paid, in one query: a series per way to pay, and
    // one more for anything else.
    let by_method = report
        .measure_by(
            Order::query(),
            placed,
            "method",
            &METHODS,
            Metric::Count,
            uow,
        )
        .await?;
    let kept = || Order::query().where_(Order::STATUS.ne(Status::Cancelled.key()));
    let total = report
        .measure(kept(), placed, Metric::Sum("total"), uow)
        .await?;
    let refunded = report
        .measure(kept(), placed, Metric::Sum("refunded"), uow)
        .await?;
    let whole = |value: Option<f64>| value.unwrap_or(0.0).round() as i64;
    let mut out = Sales::empty(report);
    for (day, orders) in out.orders.iter_mut().enumerate() {
        let counted: i64 = by_method
            .iter()
            .map(|series| whole(series.get(day).copied().flatten()))
            .sum();
        *orders = u64::try_from(counted).unwrap_or(0);
    }
    for (day, revenue) in out.revenue.iter_mut().enumerate() {
        *revenue =
            whole(total.get(day).copied().flatten()) - whole(refunded.get(day).copied().flatten());
    }
    for (at, method) in out.methods.iter_mut().enumerate() {
        let counted: i64 = by_method
            .get(at)
            .map_or(0, |series| series.iter().map(|value| whole(*value)).sum());
        *method = u64::try_from(counted).unwrap_or(0);
    }
    Ok(out)
}

/// The shelves running low, emptiest first, and today's top sellers.
async fn shelf_lists(uow: &Uow) -> Result<(Vec<Product>, Vec<Seller>)> {
    let products: Vec<Product> = inner(Product::query().get(uow).await?.into_vec());
    let today: Vec<i64> = inner(
        Order::query()
            .where_(Order::PLACED_ON.eq(day_of(now())))
            .where_(Order::STATUS.ne(Status::Cancelled.key()))
            .get(uow)
            .await?
            .into_vec(),
    )
    .into_iter()
    .map(|order| order.id)
    .collect();
    let lines: Vec<Line> = inner(
        Line::query()
            .where_(Line::ORDER_ID.is_in(today))
            .get(uow)
            .await?
            .into_vec(),
    );
    Ok((low_stock(&products), top_sellers(&products, &lines)))
}

/// The feed's rows: what happened in the last day, newest first; with a cursor,
/// only what came after it. How many there are in all, without one.
async fn feed(uow: &Uow, after: Option<Cursor>, limit: u64) -> Result<(Vec<FeedEntry>, u64)> {
    let since = now() - 24 * 3600;
    let from = after.map_or(since, |cursor| cursor.at.max(since));
    let query = Event::query().where_(Event::AT.gte(from));
    let total = match after {
        Some(_) => 0,
        None => query.clone().count(uow).await?,
    };
    // Events in the cursor's own second that came before it are left out here,
    // so a few more are read than are kept.
    let events: Vec<Event> = inner(
        query
            .order_by_desc("at")
            .order_by_desc("id")
            .limit(limit + 64)
            .get(uow)
            .await?
            .into_vec(),
    )
    .into_iter()
    .filter(|event| after.is_none_or(|cursor| (event.at, event.id) > (cursor.at, cursor.id)))
    .take(usize::try_from(limit).unwrap_or(usize::MAX))
    .collect();
    let ids: Vec<i64> = events
        .iter()
        .map(|event| event.order_id)
        .filter(|id| *id > 0)
        .collect();
    let orders: Vec<Order> = inner(Order::query().find_many(uow, ids).await?.into_vec());
    Ok((
        events
            .into_iter()
            .map(|event| FeedEntry::of(event, &orders))
            .collect(),
        total,
    ))
}

/// An order with what its drawer draws at once: the order, its lines, and how
/// many orders its customer has placed.
async fn order_view(uow: &Uow, id: i64) -> Result<Option<OrderView>> {
    let Some(order) = Order::query().find(uow, id).await? else {
        return Ok(None);
    };
    let order = order.into_inner();
    let lines = inner(
        Line::query()
            .where_(Line::ORDER_ID.eq(id))
            .order_by("id")
            .get(uow)
            .await?
            .into_vec(),
    );
    let orders = Order::query()
        .where_(Order::CUSTOMER_ID.eq(order.customer_id))
        .count(uow)
        .await?;
    let customer = Customer::query()
        .find(uow, order.customer_id)
        .await?
        .map(|record| record.into_inner())
        .unwrap_or_default();
    Ok(Some(OrderView {
        order,
        lines,
        orders,
        since: customer.since,
        phone: customer.phone,
        now: now(),
    }))
}

/// An order's events, oldest first.
async fn history(uow: &Uow, id: i64) -> Result<Vec<Event>> {
    Ok(inner(
        Event::query()
            .where_(Event::ORDER_ID.eq(id))
            .order_by("at")
            .order_by("id")
            .get(uow)
            .await?
            .into_vec(),
    ))
}

/// The customer's five latest orders other than `order`.
async fn others(uow: &Uow, order: &Order) -> Result<Vec<Order>> {
    Ok(inner(
        Order::query()
            .where_(Order::CUSTOMER_ID.eq(order.customer_id))
            .where_(Order::ID.ne(order.id))
            .order_by_desc("placed_at")
            .limit(5)
            .get(uow)
            .await?
            .into_vec(),
    ))
}

/// How many orders hold each status.
async fn status_counts(uow: &Uow) -> Result<Vec<(Status, u64)>> {
    let mut out = Vec::with_capacity(Status::ALL.len());
    for status in Status::ALL {
        let n = Order::query()
            .where_(Order::STATUS.eq(status.key()))
            .count(uow)
            .await?;
        out.push((status, n));
    }
    Ok(out)
}

/// An order that isn't in the shop.
fn missing(shell: &AppShell) -> Response {
    shell
        .page("No such order")
        .crumb("Orders", "/orders")
        .body(
            Empty::new("That order isn't in the shop")
                .icon(icons::SEARCH_X)
                .description("Search the orders for its number or the customer's name.")
                .actions(Button::outline("Back to orders").href("/orders")),
        )
        .to_view()
        .status(StatusCode::NOT_FOUND)
        .into_response()
}

/// An HTML fragment for a region.
fn fragment(title: &'static str, body: impl IntoNodes) -> View {
    View::new(title).body(body)
}

#[derive(Deserialize, Default)]
struct AfterQuery {
    after: Option<String>,
}

#[derive(Deserialize, Default)]
struct StepQuery {
    kind: Option<String>,
    order: Option<i64>,
}

impl Pages {
    /// `GET /`.
    async fn dashboard(&self, shell: AppShell, uow: Uow) -> Result<AppPage> {
        let counts = count_figures(&uow).await?;
        let today = today();
        let report = sales_report(today);
        let sold = sales(&uow, &report).await?;
        let (entries, total) = feed(&uow, None, 30).await?;
        let (low, sellers) = shelf_lists(&uow).await?;
        Ok(shell
            .page("Dashboard")
            .actions(
                Button::outline("Orders")
                    .icon(icons::SHOPPING_BAG)
                    .href("/orders"),
            )
            .body(dashboard_body(
                &counts, &report, &sold, &entries, total, &low, &sellers, today,
            )))
    }

    /// `GET /fragments/activity?after=`: only the rows newer than the cursor, as
    /// HTML, with the newest cursor in `X-UI-Cursor`.
    async fn activity(&self, uow: Uow, query: AfterQuery) -> Result<Response> {
        let (entries, _) = feed(&uow, query.after.as_deref().and_then(Cursor::parse), 100).await?;
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
            && let Ok(value) = HeaderValue::from_str(&Cursor::of(&newest.event).to_string())
        {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-ui-cursor"), value);
        }
        Ok(response)
    }

    /// `GET /orders`.
    async fn orders(
        &self,
        shell: AppShell,
        uow: Uow,
        query: TableQuery<OrderRow>,
    ) -> Result<AppPage> {
        let page = query
            .load_as(Order::query(), &uow, |record| {
                OrderRow::from(record.into_inner())
            })
            .await?;
        let statuses = status_counts(&uow).await?;
        Ok(shell.page("Orders").crumb("Dashboard", "/").body((
            orders_table(&query, &page, &statuses, today()),
            empty_drawer(),
        )))
    }

    /// `GET /orders/{id}`.
    async fn show(&self, shell: AppShell, uow: Uow, id: i64) -> Result<Response> {
        let Some(view) = order_view(&uow, id).await? else {
            return Ok(missing(&shell));
        };
        let history = history(&uow, id).await?;
        let others = others(&uow, &view.order).await?;
        let parts = Parts::Inline {
            history: &history,
            others: &others,
        };
        Ok(shell
            .page(format!("Order {}", view.order.number))
            .crumb("Dashboard", "/")
            .crumb("Orders", "/orders")
            .body(order_page(&view, today(), parts))
            .into_response())
    }

    /// `GET /orders/{id}/peek`: the order in the drawer, opened.
    async fn peek(&self, uow: Uow, id: i64) -> Result<Reply> {
        let Some(view) = order_view(&uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/orders")
                .toast(ToastTone::Warning, "That order isn't in the shop any more."));
        };
        Ok(Reply::ops_or_redirect(format!("/orders/{id}"))
            .morph(DRAWER, order_drawer(&view, today(), Parts::Lazy))
            .op(Op::Open {
                target: DRAWER.to_owned(),
            }))
    }

    /// What an action answers: the dialog closes, the drawer and the page show the
    /// order as it is now, and a toast says what happened or why not. With
    /// JavaScript off, the order's page with the toast.
    async fn answer(
        &self,
        uow: &Uow,
        id: i64,
        dialog: &'static str,
        outcome: std::result::Result<Done, (Option<Order>, Refusal)>,
    ) -> Result<Response> {
        let (tone, title, text, status) = match outcome {
            Ok(done) => {
                self.hub.nudge().await;
                (ToastTone::Success, "Done", done.text, StatusCode::OK)
            }
            Err((order, refusal)) => {
                let number = order.map_or_else(String::new, |order| order.number);
                (
                    ToastTone::Warning,
                    "Not allowed",
                    refusal.message(&number),
                    StatusCode::CONFLICT,
                )
            }
        };
        let done = status == StatusCode::OK;
        let mut reply = Reply::ops_or_redirect(format!("/orders/{id}")).op(Op::Close {
            target: dialog.to_owned(),
        });
        if let Some(view) = order_view(uow, id).await? {
            let today = today();
            let history = history(uow, id).await?;
            let others = others(uow, &view.order).await?;
            reply = reply
                .morph(DRAWER, order_drawer(&view, today, Parts::Lazy))
                .morph(
                    PAGE,
                    order_page(
                        &view,
                        today,
                        Parts::Inline {
                            history: &history,
                            others: &others,
                        },
                    ),
                );
            // A morph keeps what was typed; after a refund, the form starts again
            // from what is left.
            if done {
                reply = reply.op(Op::replace("refund", &refund_dialog(&view).into()));
            }
        }
        Ok(reply
            .toast_with(Toast::new(tone, text).title(title))
            .status(status)
            .into_response())
    }

    /// `POST /orders/{id}/refund`.
    async fn refund(
        &self,
        uow: Uow,
        id: i64,
        sent: std::result::Result<Submit<RefundForm>, Rejected<RefundForm>>,
    ) -> Result<Response> {
        let form = match sent {
            Ok(Submit(form)) => form,
            Err(rejected) => return Ok(rejected.into_response()),
        };
        let hub = self.hub.clone();
        let outcome = shop::refund(&uow, id, form.amount.0, form.reason.words(), now(), || {
            hub.event_id()
        })
        .await
        .map_err(|error| AppError::internal(error.to_string()))?;
        self.answer(&uow, id, "refund", outcome).await
    }

    /// `POST /orders/{id}/cancel`.
    async fn cancel(&self, uow: Uow, id: i64) -> Result<Response> {
        let hub = self.hub.clone();
        let outcome = shop::cancel(&uow, id, now(), || hub.event_id())
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        self.answer(&uow, id, "cancel", outcome).await
    }

    /// `POST /orders/{id}/resend`.
    async fn resend(&self, uow: Uow, id: i64) -> Result<Response> {
        let hub = self.hub.clone();
        let outcome = shop::resend(&uow, id, now(), || hub.event_id())
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        self.answer(&uow, id, "resend", outcome).await
    }

    /// `GET /inventory`.
    async fn inventory(
        &self,
        shell: AppShell,
        uow: Uow,
        query: TableQuery<ProductRow>,
    ) -> Result<AppPage> {
        let page = query
            .load_as(Product::query(), &uow, |record| {
                ProductRow::from(record.into_inner())
            })
            .await?;
        Ok(shell
            .page("Inventory")
            .crumb("Dashboard", "/")
            .body((
                inventory_table(&query, &page),
                // Where a native client's tap on a row opens the product.
                crate::views::native_only(empty_sheet()),
            )))
    }

    /// `GET /inventory/{id}/peek`: the product in a sheet, opened. With
    /// JavaScript off, the inventory.
    async fn product_peek(&self, uow: Uow, id: i64) -> Result<Reply> {
        let Some(product) = Product::query().find(&uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/inventory")
                .toast(ToastTone::Warning, "That product isn't on the shelves any more."));
        };
        Ok(Reply::ops_or_redirect("/inventory")
            .morph(inventory::SHEET, product_sheet(product.into_inner()))
            .op(Op::Open {
                target: inventory::SHEET.to_owned(),
            }))
    }

    /// `POST /inventory/{id}/restock`.
    async fn restock(
        &self,
        uow: Uow,
        headers: &HeaderMap,
        id: i64,
        sent: std::result::Result<Submit<RestockForm>, Rejected<RestockForm>>,
    ) -> Result<Response> {
        let back = headers
            .get(http::header::REFERER)
            .and_then(|value| value.to_str().ok())
            .and_then(|referer| {
                referer
                    .find("/inventory")
                    .map(|at| referer[at..].to_owned())
            })
            .unwrap_or_else(|| "/inventory".to_owned());
        let reply = Reply::ops_or_redirect(back);
        let quantity = match sent {
            Ok(Submit(form)) => form.quantity,
            // A number that isn't one, or out of range: the same words either way.
            Err(_) => 0,
        };
        if let Err(refusal) = shop::check_restock(quantity) {
            return Ok(reply
                .toast_with(
                    Toast::new(ToastTone::Warning, refusal.message("")).title("Not allowed"),
                )
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .into_response());
        }
        let event = self
            .hub
            .restock(&uow, id, quantity, now(), "")
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let Some(event) = event else {
            return Ok(reply
                .toast(ToastTone::Warning, Refusal::Missing.message(""))
                .status(StatusCode::NOT_FOUND)
                .into_response());
        };
        self.hub.nudge().await;
        let stock = Product::query()
            .find(&uow, id)
            .await?
            .map_or(0, |product| product.into_inner().stock);
        let reply = if crate::views::asked_by_native() {
            // The sheet the form was sent from has done its work.
            reply.op(Op::Close {
                target: inventory::SHEET.to_owned(),
            })
        } else {
            reply
        };
        Ok(reply
            .toast_with(
                Toast::new(
                    ToastTone::Success,
                    format!(
                        "{} of {} added. {} on the shelf now.",
                        units(quantity),
                        event.detail,
                        stock
                    ),
                )
                .title("Restocked"),
            )
            .into_response())
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
                move |shell: AppShell, uow: Uow| {
                    let me = me.clone();
                    async move { me.dashboard(shell, uow).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/activity", {
                let me = me.clone();
                move |uow: Uow, Query(query): Query<AfterQuery>| {
                    let me = me.clone();
                    async move { me.activity(uow, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/figures", |uow: Uow| async move {
                Ok::<_, AppError>(fragment("Figures", figures(&count_figures(&uow).await?)))
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/sales", |uow: Uow| async move {
                let report = sales_report(today());
                let sold = sales(&uow, &report).await?;
                Ok::<_, AppError>(fragment("Sales", sales_charts(&report, &sold)))
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/shelves", |uow: Uow| async move {
                let (low, sellers) = shelf_lists(&uow).await?;
                Ok::<_, AppError>(fragment("Shelves", shelves(&low, &sellers)))
            })
            .public(),
            RouteDef::on(Method::GET, "!/orders", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, query: TableQuery<OrderRow>| {
                    let me = me.clone();
                    async move { me.orders(shell, uow, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/orders/{id}", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.show(shell, uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/orders/{id}/steps", |uow: Uow, Path(id): Path<i64>| async move {
                let Some(order) = Order::query().find(&uow, id).await? else {
                    return Ok::<_, AppError>(StatusCode::NOT_FOUND.into_response());
                };
                let order = order.into_inner();
                let history = history(&uow, id).await?;
                Ok(fragment("History", steps(&order, &history, 4)).into_response())
            })
            .public(),
            RouteDef::on(Method::GET, "!/orders/{id}/others", |uow: Uow, Path(id): Path<i64>| async move {
                let Some(order) = Order::query().find(&uow, id).await? else {
                    return Ok::<_, AppError>(StatusCode::NOT_FOUND.into_response());
                };
                let order = order.into_inner();
                let others = others(&uow, &order).await?;
                Ok(fragment("Other orders", other_orders(&others, today())).into_response())
            })
            .public(),
            RouteDef::on(Method::GET, "!/orders/{id}/peek", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.peek(uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/orders/{id}/refund", {
                let me = me.clone();
                move |uow: Uow,
                      Path(id): Path<i64>,
                      sent: std::result::Result<Submit<RefundForm>, Rejected<RefundForm>>| {
                    let me = me.clone();
                    async move { me.refund(uow, id, sent).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/orders/{id}/cancel", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>, sent: std::result::Result<Submit<ConfirmForm>, Rejected<ConfirmForm>>| {
                    let me = me.clone();
                    async move {
                        if let Err(rejected) = sent {
                            return Ok(rejected.into_response());
                        }
                        me.cancel(uow, id).await
                    }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/orders/{id}/resend", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>, sent: std::result::Result<Submit<ConfirmForm>, Rejected<ConfirmForm>>| {
                    let me = me.clone();
                    async move {
                        if let Err(rejected) = sent {
                            return Ok(rejected.into_response());
                        }
                        me.resend(uow, id).await
                    }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/inventory", {
                let me = me.clone();
                move |shell: AppShell, uow: Uow, query: TableQuery<ProductRow>| {
                    let me = me.clone();
                    async move { me.inventory(shell, uow, query).await }
                }
            })
            .public(),
            RouteDef::on(Method::GET, "!/inventory/{id}/peek", {
                let me = me.clone();
                move |uow: Uow, Path(id): Path<i64>| {
                    let me = me.clone();
                    async move { me.product_peek(uow, id).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/inventory/{id}/restock", {
                let me = me.clone();
                move |uow: Uow,
                      headers: HeaderMap,
                      Path(id): Path<i64>,
                      sent: std::result::Result<Submit<RestockForm>, Rejected<RestockForm>>| {
                    let me = me.clone();
                    async move { me.restock(uow, &headers, id, sent).await }
                }
            })
            .public(),
            RouteDef::on(Method::POST, "!/sign-out", || async {
                Reply::redirect("/").toast(ToastTone::Info, "This demo has no accounts to sign out of.")
            })
            .public(),
        ];
        if self.settings.test_hooks {
            routes.extend(test_hooks(&me));
        }
        routes
    }
}

/// What the browser tests drive the app with, when `STORE_TEST_HOOKS` is set: one
/// step of a kind they choose (`?kind=shipped`, `&order=7` for which), and the shop
/// written afresh.
fn test_hooks(me: &Arc<Pages>) -> Vec<RouteDef> {
    vec![
        RouteDef::on(Method::POST, "!/__test/step", {
            let me = me.clone();
            move |Query(query): Query<StepQuery>| {
                let me = me.clone();
                async move {
                    let kind = query.kind.as_deref().and_then(Kind::parse);
                    let event = me
                        .hub
                        .step(&me.db, now(), kind, query.order)
                        .await
                        .map_err(|error| AppError::internal(error.to_string()))?;
                    Ok::<_, AppError>(event.map_or_else(String::new, |event| {
                        format!("{} {} {}", event.id, event.order_id, event.product_id)
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
                    let fresh = async {
                        me.hub.create(&me.db).await?;
                        me.hub.seed(&me.db, now()).await
                    };
                    fresh.await.map_err(|error| {
                        eprintln!("store-demo: reset failed: {error}");
                        AppError::internal(error.to_string())
                    })?;
                    me.hub.nudge().await;
                    Ok::<_, AppError>(StatusCode::NO_CONTENT)
                }
            }
        })
        .public(),
    ]
}
