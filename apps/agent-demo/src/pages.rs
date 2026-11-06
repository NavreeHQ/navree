//! The shop's pages: the assistant, a conversation, the orders and an order.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /` | goes to the assistant |
//! | `GET /assistant` | a new conversation: suggestions, the composer, earlier conversations |
//! | `GET /assistant/{id}` | a conversation, live, with the approval's place and the composer |
//! | `GET /orders` | the newest orders the reader may see |
//! | `GET /orders/{id}` | one order, and a composer that tells the assistant about it |
//! | `POST /__test/script` | with `AGENT_TEST_HOOKS`: what the fake model answers next |
//!
//! The assistant's own routes (sending, stopping, approving, the buttons' posts) are
//! mounted by `navree::ai::ui`, under `/assistant`.

use std::sync::Arc;

use http::Method;
use navree::access::{Accessible, CurrentAccess, Read};
use navree::ai::ui::{Assistant, PageOptions};
use navree::ai::{Ai, FakeReply};
use navree::db::Db;
use navree::http::{AppError, Controller, Json, Path, RouteDef};
use navree::injectable;
use navree::orm::Model as _;
use navree::orm::access::AccessibleQuery;
use navree::session::Session;
use navree::ui::genui::Suggestions;
use navree::ui::{
    AppShell, Badge, Collapsible, FactCard, FactGrid, Reply, Stack, Table, TableCell, TableHead,
    TableRowEl, Text, el, icons,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::Settings;
use crate::assistant::ShopAssistant;
use crate::shop::{Order, SAMPLE_ORDER, rupees};

/// The shop's pages.
#[injectable]
pub struct Pages {
    assistant: Arc<Assistant<ShopAssistant>>,
    db: Arc<Db>,
    ai: Arc<Ai>,
    settings: Arc<Settings>,
}

/// One answer of the fake model, as a test scripts it.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scripted {
    Tool { tool: String, arguments: Value },
    Text { text: String },
}

/// The orders `access`'s reader may read, newest first.
async fn readable(db: &Db, access: &CurrentAccess) -> Result<Vec<Order>, AppError> {
    let mut orders = Accessible::<Order, Read>::new(access.ability.clone())
        .apply(Order::query())
        .get(db)
        .await?
        .into_models();
    orders.sort_by_key(|order| std::cmp::Reverse(order.placed_at));
    Ok(orders)
}

/// The suggestions a new conversation starts from.
fn suggestions(token: &str) -> Suggestions {
    Suggestions::new("/assistant")
        .csrf(token.to_owned())
        .suggestion("How did UPI do this week?")
        .suggestion(format!("Show order {SAMPLE_ORDER}"))
        .suggestion(format!("Refund order {SAMPLE_ORDER}"))
}

/// What the assistant is for, above a new conversation.
fn intro() -> Text {
    Text::muted(
        "Ask about orders and payments. The assistant answers with charts and cards drawn \
         from what you may see, and asks before it refunds anything.",
    )
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = vec![
            RouteDef::on(Method::GET, "!/", || async {
                Reply::redirect("/assistant")
            })
            .public(),
        ];

        let me = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                "!/assistant",
                move |shell: AppShell, access: CurrentAccess, session: Session| {
                    let me = me.clone();
                    async move {
                        let user = access.user.ok_or_else(AppError::unauthenticated)?;
                        let token = session.csrf_token();
                        let threads = me.assistant.threads(user, None).await?;
                        let composer = me.assistant.new_composer(
                            user,
                            PageOptions {
                                csrf: Some(token.clone()),
                                ..PageOptions::default()
                            },
                        );
                        Ok::<_, AppError>(shell.page("Assistant").body(Stack::gap(4).children((
                            intro(),
                            suggestions(&token),
                            composer,
                            Collapsible::new("Earlier conversations").body(threads),
                        ))))
                    }
                },
            )
            .authenticated()
            .page()
            .name("shop.assistant"),
        );

        let me = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                "!/assistant/{conversation}",
                move |shell: AppShell,
                      access: CurrentAccess,
                      session: Session,
                      Path(conversation): Path<Uuid>| {
                    let me = me.clone();
                    async move {
                        let user = access.user.ok_or_else(AppError::unauthenticated)?;
                        let token = session.csrf_token();
                        let page = me
                            .assistant
                            .page(
                                access.0.clone(),
                                conversation,
                                PageOptions {
                                    csrf: Some(token),
                                    ..PageOptions::default()
                                },
                            )
                            .await?;
                        let threads = me.assistant.threads(user, Some(conversation)).await?;
                        Ok::<_, AppError>(
                            shell
                                .page("Assistant")
                                .crumb("Assistant", "/assistant")
                                .body(Stack::gap(4).children((
                                    page.conversation,
                                    page.approval,
                                    page.composer,
                                    Collapsible::new("Other conversations").body(threads),
                                ))),
                        )
                    }
                },
            )
            .authenticated()
            .page()
            .name("shop.conversation"),
        );

        let me = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                "!/orders",
                move |shell: AppShell, access: CurrentAccess| {
                    let me = me.clone();
                    async move {
                        let orders = readable(&me.db, &access).await?;
                        let rows: Vec<TableRowEl> = orders
                            .iter()
                            .take(50)
                            .map(|order| {
                                TableRowEl::new((
                                    TableCell::new(
                                        el("a")
                                            .href(format!("/orders/{}", order.id))
                                            .child(order.number.clone()),
                                    ),
                                    TableCell::new(format!("{} · {}", order.customer, order.city)),
                                    TableCell::new(order.method_label()),
                                    TableCell::new(rupees(order.total)).numeric(),
                                    TableCell::new(Badge::new(order.status_label())),
                                ))
                            })
                            .collect();
                        let table = Table::new()
                            .caption("The newest orders")
                            .head((
                                TableHead::new("Order"),
                                TableHead::new("Customer"),
                                TableHead::new("Paid by"),
                                TableHead::new("Total").numeric(),
                                TableHead::new("Status"),
                            ))
                            .rows(rows);
                        Ok::<_, AppError>(shell.page("Orders").body(table))
                    }
                },
            )
            .authenticated()
            .page()
            .name("shop.orders"),
        );

        let me = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                "!/orders/{id}",
                move |shell: AppShell,
                      access: CurrentAccess,
                      session: Session,
                      Path(id): Path<i64>| {
                    let me = me.clone();
                    async move {
                        let user = access.user.ok_or_else(AppError::unauthenticated)?;
                        let Some(order) = readable(&me.db, &access)
                            .await?
                            .into_iter()
                            .find(|order| order.id == id)
                        else {
                            return Err(AppError::not_found());
                        };
                        let token = session.csrf_token();
                        // The composer sends what this page is about, signed for this reader:
                        // the assistant hears of the order only as the server reads it.
                        let composer = me.assistant.new_composer(
                            user,
                            PageOptions {
                                csrf: Some(token),
                                context: Some(("order".to_owned(), order.id.to_string())),
                                draft: Some(format!(
                                    "What should I know about order {}?",
                                    order.number
                                )),
                                ..PageOptions::default()
                            },
                        );
                        Ok::<_, AppError>(
                            shell
                                .page(format!("Order {}", order.number))
                                .crumb("Orders", "/orders")
                                .body(
                                    Stack::gap(4).children((
                                        FactGrid::new().cols(2).cards([
                                            FactCard::new(
                                                "Customer",
                                                format!("{} · {}", order.customer, order.city),
                                            )
                                            .icon(icons::USER),
                                            FactCard::new("Total", rupees(order.total))
                                                .icon(icons::INDIAN_RUPEE),
                                            FactCard::new("Paid by", order.method_label())
                                                .icon(icons::WALLET),
                                            FactCard::new("Status", order.status_label())
                                                .icon(icons::PACKAGE),
                                        ]),
                                        Text::muted("Ask the assistant about this order."),
                                        composer,
                                    )),
                                ),
                        )
                    }
                },
            )
            .authenticated()
            .page()
            .name("shop.order"),
        );

        if self.settings.test_hooks {
            let me = self.clone();
            routes.push(
                RouteDef::on(
                    Method::POST,
                    "!/__test/script",
                    move |Json(replies): Json<Vec<Scripted>>| {
                        let me = me.clone();
                        async move {
                            let Some(fake) = me.ai.fake() else {
                                return Err(AppError::conflict("the app runs a real model"));
                            };
                            fake.respond_with(replies.into_iter().map(|reply| match reply {
                                Scripted::Tool { tool, arguments } => {
                                    FakeReply::tool_call(tool, arguments)
                                }
                                Scripted::Text { text } => FakeReply::text(text),
                            }));
                            Ok::<_, AppError>(Reply::ops(Vec::new()))
                        }
                    },
                )
                .public()
                .name("shop.test.script"),
            );
        }

        routes
    }
}
