//! The shop's assistant: an agent with two tools, and the components it may choose.
//!
//! - `find_orders` (read only) looks orders up for the reader.
//! - `refund_order` needs the reader's approval: the run stops, the page draws its card
//!   in a place of its own, and only a manager is offered it.
//! - `payments_by_method` draws a donut of the period's payments, loaded under the
//!   reader's access when the page is drawn: the model chooses the period, never the
//!   numbers.
//! - `order_card` draws one order, with Mark delivered and Refund for a manager. Refund
//!   asks first, in a dialog the page draws.

use chrono::Utc;
use navree::access::{Read, Update};
use navree::ai::ui::genui::{
    Action, ActionButton, ActionDone, AppError, BoxFuture, Catalog, Key, RunAction, Surface,
    SurfaceView,
};
use navree::ai::ui::{AssistantConfig, PageContext};
use navree::ai::{Agent, AgentOptions, AgentRun, agent};
use navree::injectable;
use navree::openapi::ToSchema;
use navree::orm::Model as _;
use navree::orm::access::AccessibleQuery;
use navree::ui::charts::{Donut, NumberFormat};
use navree::ui::{FactCard, FactGrid, Text, icons};
use navree::validation::Validate;
use serde::{Deserialize, Serialize};

use crate::shop::{METHODS, Order, SHOP, ist, method_label, period_start, rupees};

/// Words from an order to look for.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct FindOrders {
    /// Words from the customer's name, the city or the order's number, such as
    /// "Harpreet", "Ludhiana" or "KK-1042". Leave out for the newest orders.
    #[validate(length(max = 60))]
    pub search: Option<String>,
}

/// The order to refund.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct RefundOrder {
    /// The order's number, such as "KK-1042".
    #[validate(length(min = 1, max = 20))]
    pub order: String,
}

/// An order, as the model reads it.
#[derive(Debug, Serialize)]
pub struct OrderLine {
    pub number: String,
    pub customer: String,
    pub city: String,
    pub total: String,
    pub paid_by: &'static str,
    pub status: &'static str,
}

impl From<&Order> for OrderLine {
    fn from(order: &Order) -> Self {
        Self {
            number: order.number.clone(),
            customer: order.customer.clone(),
            city: order.city.clone(),
            total: rupees(order.total),
            paid_by: order.method_label(),
            status: order.status_label(),
        }
    }
}

/// The orders `run`'s reader may read, newest first.
async fn readable(run: &AgentRun) -> Result<Vec<Order>, AppError> {
    let mut orders = run
        .accessible::<Order, Read>()
        .apply(Order::query())
        .get(run.uow())
        .await?
        .into_models();
    orders.sort_by_key(|order| std::cmp::Reverse(order.placed_at));
    Ok(orders)
}

/// The order with this number (`KK-1042`, or `1042`), if the reader may read it.
async fn by_number(run: &AgentRun, number: &str) -> Result<Order, AppError> {
    let wanted = number.trim().to_ascii_uppercase();
    let wanted = if wanted.starts_with("KK-") {
        wanted
    } else {
        format!("KK-{wanted}")
    };
    readable(run)
        .await?
        .into_iter()
        .find(|order| order.number == wanted)
        .ok_or_else(|| AppError::not_found().with_detail(format!("No order {wanted}.")))
}

/// The shop's assistant.
#[injectable]
pub struct ShopAssistant;

#[agent]
impl Agent for ShopAssistant {
    const NAME: &'static str = "shop-assistant";

    fn instructions(&self, run: &AgentRun) -> String {
        format!(
            "You are the assistant of {SHOP}, a kitchenware shop in Ludhiana that sells across \
             India, talking to {}. Today is {} in India (IST). Amounts are in rupees. When asked \
             how payments did, show `payments_by_method` for the period. To look at one order, \
             show `order_card`. To refund an order, call `refund_order`: the user approves it on \
             the page. Answer in one or two short sentences.",
            run.user().name,
            Utc::now().with_timezone(&ist()).format("%A %-d %B %Y"),
        )
    }

    fn options(&self) -> AgentOptions {
        AgentOptions {
            max_steps: 6,
            ..AgentOptions::default()
        }
    }

    /// Finds the orders the user can see, newest first: number, customer, city, total in
    /// rupees, how it was paid and where it stands. At most ten.
    #[tool(read_only)]
    async fn find_orders(
        &self,
        run: &AgentRun,
        args: FindOrders,
    ) -> Result<Vec<OrderLine>, AppError> {
        let words = args.search.unwrap_or_default().to_lowercase();
        Ok(readable(run)
            .await?
            .iter()
            .filter(|order| {
                words.is_empty()
                    || order.customer.to_lowercase().contains(&words)
                    || order.city.to_lowercase().contains(&words)
                    || order.number.to_lowercase().contains(&words)
            })
            .take(10)
            .map(OrderLine::from)
            .collect())
    }

    /// Refunds what is left of an order to the customer, by the way they paid. The user
    /// approves it on the page before it runs.
    #[tool(needs_approval)]
    #[can(Update, Order)]
    async fn refund_order(&self, run: &AgentRun, args: RefundOrder) -> Result<OrderLine, AppError> {
        let order = by_number(run, &args.order).await?;
        refund(run, order.id).await
    }
}

/// Refunds what is left of order `id`, checked under `run`'s access.
async fn refund(run: &AgentRun, id: i64) -> Result<OrderLine, AppError> {
    let mut order = run.authorize::<Order, Update>(id).await?.into_inner();
    if order.left() == 0 {
        return Err(AppError::conflict(format!(
            "Order {} has nothing left to refund.",
            order.number
        )));
    }
    order
        .update(run.uow(), |order| {
            order.refunded = order.total;
            order.status = "refunded".to_owned();
        })
        .await?;
    Ok(OrderLine::from(&*order))
}

navree::ui::choices! {
    /// The days the payments cover, in IST.
    pub enum Period {
        /// Today so far.
        Today = "today",
        /// Monday to now.
        ThisWeek = "this_week",
        /// The first of the month to now.
        ThisMonth = "this_month",
    }
}

navree::ui::props! {
    /// How the shop's payments did over a period, split by how customers paid (UPI, card,
    /// net banking, cash on delivery), as a donut with the total in the middle.
    pub struct PaymentsProps {
        /// The period.
        pub period: Period,
    }
}

navree::ui::props! {
    /// One order: its customer, total, how it was paid and where it stands, with what
    /// the user may do to it.
    pub struct OrderProps {
        /// The order's number, such as "KK-1042".
        pub order: Key<20>,
    }
}

navree::ui::props! {
    /// An order a button acts on.
    pub struct OrderInput {
        /// The order's id.
        pub order: Key<20>,
    }
}

/// The donut of payments by method.
pub struct PaymentsByMethod;

impl Surface<AgentRun> for PaymentsByMethod {
    type Props = PaymentsProps;
    const NAME: &'static str = "payments_by_method";
    const CAN: Option<(&'static str, &'static str)> = Some(("read", "Order"));

    fn title(&self, props: &PaymentsProps) -> String {
        match props.period {
            Period::Today => "Payments today".to_owned(),
            Period::ThisWeek => "Payments this week".to_owned(),
            Period::ThisMonth => "Payments this month".to_owned(),
        }
    }

    fn render<'a>(
        &'a self,
        props: &'a PaymentsProps,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move {
            let since = period_start(props.period.as_str(), Utc::now());
            let orders: Vec<Order> = readable(run)
                .await?
                .into_iter()
                .filter(|order| order.placed_at >= since)
                .collect();
            let mut donut = Donut::new(self.title(props))
                .format(NumberFormat::currency("₹"))
                .locale("en-IN");
            let mut total = 0;
            for method in METHODS {
                let paid: i64 = orders
                    .iter()
                    .filter(|order| order.method == method)
                    .map(Order::left)
                    .sum();
                total += paid;
                donut = donut.part(method_label(method), paid as f64 / 100.0);
            }
            let donut = donut.center(rupees(total), format!("{} orders", orders.len()));
            Ok(SurfaceView::new(donut))
        })
    }
}

/// One order, with its buttons.
pub struct OrderCard;

impl Surface<AgentRun> for OrderCard {
    type Props = OrderProps;
    const NAME: &'static str = "order_card";
    const CAN: Option<(&'static str, &'static str)> = Some(("read", "Order"));
    const ACTIONS: &'static [&'static str] = &["mark_delivered", "refund"];

    fn title(&self, props: &OrderProps) -> String {
        format!("Order {}", props.order.as_str().to_ascii_uppercase())
    }

    fn render<'a>(
        &'a self,
        props: &'a OrderProps,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<SurfaceView, AppError>> {
        Box::pin(async move {
            let order = by_number(run, props.order.as_str()).await?;
            let grid = FactGrid::new().cols(2).cards([
                FactCard::new("Customer", format!("{} · {}", order.customer, order.city))
                    .icon(icons::USER),
                FactCard::new("Total", rupees(order.total)).icon(icons::INDIAN_RUPEE),
                FactCard::new("Paid by", order.method_label()).icon(icons::WALLET),
                FactCard::new("Status", order.status_label()).icon(icons::PACKAGE),
            ]);
            if !run.ability().can("update", "Order", None) {
                return Ok(SurfaceView::new(navree::ui::el("div").children((
                    grid,
                    Text::muted("Only a manager can change an order."),
                ))));
            }
            let mut view = SurfaceView::new(grid);
            let input = OrderInput {
                order: Key::new(&order.id.to_string()).expect("an id is a key"),
            };
            if order.status == "paid" {
                view = view.action(ActionButton::new::<MarkDelivered>("Mark delivered", &input));
            }
            if order.left() > 0 {
                let amount = rupees(order.left());
                view = view.action(
                    ActionButton::new::<RefundAction>(format!("Refund {amount}"), &input).confirm(
                        format!("Refund order {}?", order.number),
                        format!(
                            "{amount} goes back to {} by {}. This can't be taken back.",
                            order.customer,
                            order.method_label()
                        ),
                        format!("Refund {amount}"),
                    ),
                );
            }
            Ok(view)
        })
    }
}

/// Marks a paid order delivered.
pub struct MarkDelivered;

impl Action for MarkDelivered {
    type Input = OrderInput;
    const NAME: &'static str = "mark_delivered";
    const CAN: Option<(&'static str, &'static str)> = Some(("update", "Order"));
}

impl RunAction<AgentRun> for MarkDelivered {
    fn run<'a>(
        &'a self,
        input: OrderInput,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
        Box::pin(async move {
            let mut order = run
                .authorize::<Order, Update>(input.order.as_str())
                .await?
                .into_inner();
            if order.status != "paid" {
                return Err(AppError::conflict(format!(
                    "Order {} is {} already.",
                    order.number,
                    order.status_label().to_lowercase()
                )));
            }
            order
                .update(run.uow(), |order| order.status = "delivered".to_owned())
                .await?;
            Ok(
                ActionDone::new(format!("Order {} is delivered", order.number))
                    .note(format!("The user marked order {} delivered.", order.number)),
            )
        })
    }
}

/// Refunds what is left of an order: the page asks first.
pub struct RefundAction;

impl Action for RefundAction {
    type Input = OrderInput;
    const NAME: &'static str = "refund";
    const CAN: Option<(&'static str, &'static str)> = Some(("update", "Order"));
    const DESTRUCTIVE: bool = true;
}

impl RunAction<AgentRun> for RefundAction {
    fn run<'a>(
        &'a self,
        input: OrderInput,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<ActionDone, AppError>> {
        Box::pin(async move {
            let id: i64 = input
                .order
                .as_str()
                .parse()
                .map_err(|_| AppError::not_found())?;
            let line = refund(run, id).await?;
            Ok(
                ActionDone::new(format!("Refunded {} for order {}", line.total, line.number)).note(
                    format!("The user refunded order {} from its card.", line.number),
                ),
            )
        })
    }
}

/// What an order's page tells the assistant: the order, if the reader may read it.
pub struct OrderContext;

impl PageContext for OrderContext {
    fn describe<'a>(
        &'a self,
        key: &'a str,
        run: &'a AgentRun,
    ) -> BoxFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            let order = run.authorize::<Order, Read>(key).await?.into_inner();
            Ok(format!(
                "order {} by {} of {}: {} by {}, {}",
                order.number,
                order.customer,
                order.city,
                rupees(order.total),
                order.method_label(),
                order.status_label().to_lowercase()
            ))
        })
    }
}

/// The components the assistant may choose.
pub fn catalog() -> Catalog<AgentRun> {
    Catalog::new()
        .surface(PaymentsByMethod)
        .surface(OrderCard)
        .action(MarkDelivered)
        .action(RefundAction)
        .link("/orders/{key}")
}

/// How the assistant is set up on the shop's pages.
pub fn config() -> AssistantConfig {
    AssistantConfig::at("/assistant")
        .catalog(catalog())
        .tool_label("find_orders", "Looked up orders")
        .tool_label("refund_order", "Refund an order")
        .context("order", OrderContext)
}
