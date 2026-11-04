//! The shop as it runs: a simulator that places orders, moves them through the
//! warehouse and restocks shelves while the app runs, and the server-sent event
//! stream that tells every open page.
//!
//! An event on the stream is only a nudge. The page never reads what it says:
//! each live part of a page fetches its own fragment again, so the stream can't
//! put anything in a page that the server didn't draw.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use navree::db::{Db, DbError, ErrorKind, Executor as _};
use navree::http::response::sse::{Event as Sse, KeepAlive, Sse as SseResponse};
use navree::http::response::{IntoResponse, Response};
use navree::orm::Model as _;
use navree::prelude::BoxError;
use tokio::sync::broadcast;

use crate::data::{
    self, Customer, Dice, Event, Fulfilment, Kind, Level, Line, Order, Payment, Product, STAFF,
    Status, warehouse,
};
use crate::shop;

/// The names of the stream's events, one for each kind of live part.
pub const STREAM_EVENTS: [&str; 4] = ["orders", "activity", "stats", "inventory"];

/// What happens next, and who hears about it.
pub struct Hub {
    tx: broadcast::Sender<()>,
    dice: Mutex<Dice>,
    next_event: AtomicI64,
    next_order: AtomicI64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(64).0,
            dice: Mutex::new(Dice::new(2_026)),
            next_event: AtomicI64::new(1),
            next_order: AtomicI64::new(1),
        }
    }

    /// Tells every open stream that the shop moved.
    pub fn nudge(&self) {
        let _ = self.tx.send(());
    }

    /// The id the next event gets.
    pub fn event_id(&self) -> i64 {
        self.next_event.fetch_add(1, Ordering::SeqCst)
    }

    fn roll(&self, n: u64) -> u64 {
        self.dice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .below(n)
    }

    /// Creates the tables, empty and without their indexes: [`seed`](Self::seed)
    /// writes the rows first and builds the indexes once, after.
    pub async fn create(&self, db: &Db) -> Result<(), BoxError> {
        let pool = db.pool();
        for statement in [
            // One writer at a time, readers never wait for it.
            "PRAGMA journal_mode=WAL",
            "DROP TABLE IF EXISTS events",
            "DROP TABLE IF EXISTS order_lines",
            "DROP TABLE IF EXISTS orders",
            "DROP TABLE IF EXISTS customers",
            "DROP TABLE IF EXISTS products",
            "CREATE TABLE products (id INTEGER PRIMARY KEY, sku TEXT NOT NULL, name TEXT NOT NULL, category TEXT NOT NULL, icon TEXT NOT NULL, price INTEGER NOT NULL, stock INTEGER NOT NULL, reorder_at INTEGER NOT NULL, level TEXT NOT NULL)",
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, phone TEXT NOT NULL, country TEXT NOT NULL, state TEXT NOT NULL, city TEXT NOT NULL, pin TEXT NOT NULL, since DATE)",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, number TEXT NOT NULL, customer_id INTEGER NOT NULL, customer TEXT NOT NULL, email TEXT NOT NULL, country TEXT NOT NULL, state TEXT NOT NULL, city TEXT NOT NULL, pin TEXT NOT NULL, status TEXT NOT NULL, fulfilment TEXT NOT NULL, payment TEXT NOT NULL, method TEXT NOT NULL, card_brand TEXT NOT NULL, card_last4 TEXT NOT NULL, items INTEGER NOT NULL, subtotal INTEGER NOT NULL, shipping INTEGER NOT NULL, total INTEGER NOT NULL, refunded INTEGER NOT NULL, warehouse TEXT NOT NULL, carrier TEXT NOT NULL, tracking TEXT NOT NULL, note TEXT NOT NULL, placed_at INTEGER NOT NULL, placed_on DATE NOT NULL, confirmed_at INTEGER NOT NULL)",
            "CREATE TABLE order_lines (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL, product_id INTEGER NOT NULL, sku TEXT NOT NULL, name TEXT NOT NULL, icon TEXT NOT NULL, quantity INTEGER NOT NULL, price INTEGER NOT NULL)",
            "CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, at INTEGER NOT NULL, order_id INTEGER NOT NULL, product_id INTEGER NOT NULL, amount INTEGER NOT NULL, actor TEXT NOT NULL, detail TEXT NOT NULL)",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// The indexes each list reads by: every sort and filter of the orders table
    /// ends in the key, as keyset pages need; an order's lines and events, a
    /// customer's orders and the feed by time each have their own.
    pub const INDEXES: [&'static str; 15] = [
        "CREATE INDEX orders_placed ON orders (placed_on, id)",
        "CREATE INDEX orders_status ON orders (status, placed_on, id)",
        "CREATE INDEX orders_fulfilment ON orders (fulfilment, placed_on, id)",
        "CREATE INDEX orders_payment ON orders (payment, placed_on, id)",
        "CREATE INDEX orders_state ON orders (state, placed_on, id)",
        "CREATE INDEX orders_open ON orders (status, fulfilment)",
        "CREATE INDEX orders_total ON orders (total, id)",
        "CREATE INDEX orders_number ON orders (number, id)",
        "CREATE INDEX orders_customer ON orders (customer_id, placed_at)",
        "CREATE INDEX order_lines_order ON order_lines (order_id, id)",
        "CREATE INDEX events_order ON events (order_id, at)",
        "CREATE INDEX events_at ON events (at, id)",
        "CREATE INDEX products_stock ON products (stock, id)",
        "CREATE INDEX products_level ON products (level, stock, id)",
        "CREATE INDEX products_name ON products (name, id)",
    ];

    /// Writes the shop afresh at real size, the newest order a few minutes before
    /// `now`: 2,000 products, 40,000 customers and 200,000 orders with their lines
    /// and events, in one transaction, a chunk of orders at a time so the whole
    /// shop is never in memory. The indexes are built after the rows.
    pub async fn seed(&self, db: &Db, now: i64) -> Result<(), BoxError> {
        let products = data::products();
        let customers = data::customers(now);
        let orm = |error: navree::orm::OrmError| DbError::new(ErrorKind::Other, error.to_string());
        let written = db
            .transaction(|uow| async move {
                Product::query()
                    .insert(&uow, products.clone())
                    .await
                    .map_err(orm)?;
                for chunk in customers.chunks(2_000) {
                    Customer::query()
                        .insert(&uow, chunk.to_vec())
                        .await
                        .map_err(orm)?;
                }
                let mut next_event = 1;
                let mut from = 1;
                while from <= data::ORDERS {
                    let to = from + data::CHUNK - 1;
                    let (orders, lines, mut events) =
                        data::orders(from, to, now, &products, &customers);
                    for chunk in orders.chunks(1_000) {
                        Order::query()
                            .insert(&uow, chunk.to_vec())
                            .await
                            .map_err(orm)?;
                    }
                    for chunk in lines.chunks(3_000) {
                        Line::query()
                            .insert(&uow, chunk.to_vec())
                            .await
                            .map_err(orm)?;
                    }
                    for event in &mut events {
                        event.id = next_event;
                        next_event += 1;
                    }
                    for chunk in events.chunks(3_000) {
                        Event::query()
                            .insert(&uow, chunk.to_vec())
                            .await
                            .map_err(orm)?;
                    }
                    from = to + 1;
                }
                let mut shelves = data::stock_events(&products, now);
                for event in &mut shelves {
                    event.id = next_event;
                    next_event += 1;
                }
                Event::query().insert(&uow, shelves).await.map_err(orm)?;
                Ok(next_event)
            })
            .await?;
        let pool = db.pool();
        for statement in Self::INDEXES {
            pool.execute_script(statement).await?;
        }
        pool.execute_script("ANALYZE").await?;
        self.next_event.store(written, Ordering::SeqCst);
        self.next_order.store(data::ORDERS + 1, Ordering::SeqCst);
        Ok(())
    }

    /// One thing happens: `kind` when given, otherwise whatever the dice say; to
    /// order `order` when given, if it is where that can happen. Returns what
    /// happened, or `None` when nothing could.
    pub async fn step(
        &self,
        db: &Db,
        now: i64,
        kind: Option<Kind>,
        order: Option<i64>,
    ) -> Result<Option<Event>, BoxError> {
        let kind = kind.unwrap_or_else(|| match self.roll(100) {
            0..=49 => Kind::Placed,
            50..=64 => Kind::Packed,
            65..=79 => Kind::Shipped,
            80..=93 => Kind::Delivered,
            _ => Kind::Restocked,
        });
        let event = match kind {
            Kind::Placed => self.place(db, now).await?,
            Kind::Packed => self.advance(db, now, Kind::Packed, order).await?,
            Kind::Shipped => self.advance(db, now, Kind::Shipped, order).await?,
            Kind::Delivered => self.advance(db, now, Kind::Delivered, order).await?,
            Kind::Restocked => self.restock_low(db, now).await?,
            _ => None,
        };
        if event.is_some() {
            self.nudge();
        }
        Ok(event)
    }

    /// A customer places an order for what is on the shelf. Stock that falls to
    /// its reorder point says so in the feed.
    async fn place(&self, db: &Db, now: i64) -> Result<Option<Event>, BoxError> {
        let products: Vec<Product> = Product::query()
            .where_(Product::STOCK.gt(0))
            .get(db)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect();
        if products.is_empty() {
            return Ok(None);
        }
        let who =
            i64::try_from(self.roll(u64::try_from(data::CUSTOMERS).unwrap_or(1))).unwrap_or(0) + 1;
        let Some(customer) = Customer::query().find(db, who).await? else {
            return Ok(None);
        };
        let customers = [customer.into_inner()];
        let id = self.next_order.fetch_add(1, Ordering::SeqCst);
        let (mut order, mut lines, _) = data::one_order(id, now, now + 60, &products, &customers);
        // Nothing more than the shelf holds.
        for line in &mut lines {
            if let Some(product) = products.iter().find(|p| p.id == line.product_id) {
                line.quantity = line.quantity.min(product.stock);
            }
        }
        lines.retain(|line| line.quantity > 0);
        order.items = lines.iter().map(|line| line.quantity).sum();
        order.subtotal = lines.iter().map(|line| line.price * line.quantity).sum();
        order.shipping = data::shipping_for(order.subtotal, &order.country);
        order.total = order.subtotal + order.shipping;
        order.status = Status::Open.key().to_owned();
        order.fulfilment = Fulfilment::Unfulfilled.key().to_owned();
        // UPI and cards pay at once; cash on delivery pays when it arrives.
        order.payment = if order.method == "cod" {
            Payment::Pending
        } else {
            Payment::Paid
        }
        .key()
        .to_owned();
        order.refunded = 0;
        order.tracking = String::new();
        order.placed_at = now;
        order.placed_on = data::day_of(now);
        order.confirmed_at = now;
        Order::query().insert(db, [order.clone()]).await?;
        Line::query().insert(db, lines.clone()).await?;
        let placed = shop::record(
            db,
            self.event_id(),
            Event {
                kind: Kind::Placed.key().to_owned(),
                at: now,
                order_id: order.id,
                amount: order.total,
                actor: order.customer.clone(),
                ..Event::default()
            },
        )
        .await?;
        if order.payment == Payment::Paid.key() {
            shop::record(
                db,
                self.event_id(),
                Event {
                    kind: Kind::Paid.key().to_owned(),
                    at: now,
                    order_id: order.id,
                    amount: order.total,
                    actor: order.customer.clone(),
                    ..Event::default()
                },
            )
            .await?;
        }
        for line in &lines {
            let Some(product) = shop::add_stock(db, line.product_id, -line.quantity).await? else {
                continue;
            };
            let before = Level::of(product.stock + line.quantity, product.reorder_at);
            if Level::parse(&product.level) != Level::InStock && before == Level::InStock
                || Level::parse(&product.level) == Level::Out && before != Level::Out
            {
                shop::record(
                    db,
                    self.event_id(),
                    Event {
                        kind: Kind::LowStock.key().to_owned(),
                        at: now,
                        product_id: product.id,
                        amount: product.stock,
                        actor: product.name.clone(),
                        detail: product.sku.clone(),
                        ..Event::default()
                    },
                )
                .await?;
            }
        }
        Ok(Some(placed))
    }

    /// An open order takes its next step: paid, packed, shipped or delivered.
    async fn advance(
        &self,
        db: &Db,
        now: i64,
        kind: Kind,
        who: Option<i64>,
    ) -> Result<Option<Event>, BoxError> {
        let fulfilment = match kind {
            Kind::Packed => Fulfilment::Unfulfilled,
            Kind::Shipped => Fulfilment::Packed,
            Kind::Delivered => Fulfilment::Shipped,
            _ => return Ok(None),
        };
        let mut query = Order::query()
            .where_(Order::STATUS.eq(Status::Open.key()))
            .where_(Order::FULFILMENT.eq(fulfilment.key()));
        // Only a paid order is packed, or one paid in cash when it arrives.
        if kind == Kind::Packed {
            query = query.where_(
                Order::PAYMENT
                    .eq(Payment::Paid.key())
                    .or(Order::METHOD.eq("cod")),
            );
        }
        let order = match who {
            Some(id) => query.find(db, id).await?.map(|record| record.into_inner()),
            None => {
                // The oldest few wait the longest: one of them goes next.
                let waiting: Vec<Order> = query
                    .order_by("placed_at")
                    .limit(6)
                    .get(db)
                    .await?
                    .into_vec()
                    .into_iter()
                    .map(|record| record.into_inner())
                    .collect();
                let at = usize::try_from(self.roll(waiting.len().max(1) as u64)).unwrap_or(0);
                waiting.into_iter().nth(at)
            }
        };
        let Some(order) = order else {
            return Ok(None);
        };
        let house = warehouse(&order.warehouse);
        let cash = order.method == "cod";
        let (sets, actor, detail, amount) = match kind {
            Kind::Packed => (
                vec![Order::FULFILMENT.set(Fulfilment::Packed.key())],
                house.city.to_owned(),
                String::new(),
                order.items,
            ),
            Kind::Shipped => {
                let tracking = {
                    let mut dice = self.dice.lock().unwrap_or_else(PoisonError::into_inner);
                    data::tracking(&order, &mut dice)
                };
                (
                    vec![
                        Order::FULFILMENT.set(Fulfilment::Shipped.key()),
                        Order::TRACKING.set(tracking.clone()),
                    ],
                    house.city.to_owned(),
                    tracking,
                    0,
                )
            }
            _ => {
                let mut sets = vec![
                    Order::FULFILMENT.set(Fulfilment::Delivered.key()),
                    Order::STATUS.set(Status::Completed.key()),
                ];
                if cash {
                    sets.push(Order::PAYMENT.set(Payment::Paid.key()));
                }
                (sets, house.carrier.to_owned(), String::new(), 0)
            }
        };
        let changed = Order::query()
            .where_(Order::ID.eq(order.id))
            .where_(Order::FULFILMENT.eq(order.fulfilment.clone()))
            .where_(Order::PAYMENT.eq(order.payment.clone()))
            .where_(Order::STATUS.eq(Status::Open.key()))
            .update(db, sets)
            .await?;
        if changed != 1 {
            return Ok(None);
        }
        let event = shop::record(
            db,
            self.event_id(),
            Event {
                kind: kind.key().to_owned(),
                at: now,
                order_id: order.id,
                amount,
                actor: actor.clone(),
                detail,
                ..Event::default()
            },
        )
        .await?;
        // Cash on delivery: the courier takes the money at the door.
        if cash && kind == Kind::Delivered {
            shop::record(
                db,
                self.event_id(),
                Event {
                    kind: Kind::Paid.key().to_owned(),
                    at: now,
                    order_id: order.id,
                    amount: order.total,
                    actor,
                    detail: "Cash on delivery".to_owned(),
                    ..Event::default()
                },
            )
            .await?;
        }
        Ok(Some(event))
    }

    /// A supplier's delivery fills the emptiest shelf.
    async fn restock_low(&self, db: &Db, now: i64) -> Result<Option<Event>, BoxError> {
        let Some(product) = Product::query()
            .where_(Product::LEVEL.ne(Level::InStock.key()))
            .order_by("stock")
            .first(db)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(None);
        };
        let units = product.reorder_at * 3;
        self.restock(db, product.id, units, now, "A supplier's delivery")
            .await
    }

    /// Adds `units` to product `id`, by `actor`.
    pub async fn restock(
        &self,
        db: &impl navree::orm::Executor,
        id: i64,
        units: i64,
        now: i64,
        actor: &str,
    ) -> Result<Option<Event>, BoxError> {
        let Some(product) = shop::add_stock(db, id, units).await? else {
            return Ok(None);
        };
        let event = shop::record(
            db,
            self.event_id(),
            Event {
                kind: Kind::Restocked.key().to_owned(),
                at: now,
                product_id: product.id,
                amount: units,
                actor: if actor.is_empty() { STAFF } else { actor }.to_owned(),
                detail: product.name.clone(),
                ..Event::default()
            },
        )
        .await?;
        Ok(Some(event))
    }

    /// Runs the simulator: something happens every `every`, give or take a
    /// little. It runs until the task holding it is dropped.
    pub async fn simulate(self: Arc<Self>, db: Arc<Db>, every: Duration) {
        loop {
            let jitter = Duration::from_millis(self.roll(900));
            tokio::time::sleep(every + jitter).await;
            if let Err(error) = self.step(&db, now(), None, None).await {
                eprintln!("store-demo: the simulator could not take a step: {error}");
            }
        }
    }

    /// `GET /events`: the stream. Each change says it to every kind of live part:
    /// `orders` (the table), `activity` (the feed), `stats` (the figures) and
    /// `inventory` (the products).
    pub fn stream(&self, headers: &HeaderMap) -> Response {
        // The runtime asks once what a failed stream answers: say it's a stream.
        if headers.contains_key("x-ui-probe") {
            return ([(CONTENT_TYPE, "text/event-stream")], "").into_response();
        }
        let rx = self.tx.subscribe();
        // A first comment goes out at once: Firefox reports a stream open only when
        // its first bytes arrive.
        let queue = VecDeque::from([Sse::default().comment("ready")]);
        let events = futures_util::stream::unfold((rx, queue), |(mut rx, mut queue)| async move {
            loop {
                if let Some(event) = queue.pop_front() {
                    return Some((Ok::<_, Infallible>(event), (rx, queue)));
                }
                match rx.recv().await {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        for name in STREAM_EVENTS {
                            queue.push_back(Sse::default().event(name).data("changed"));
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        SseResponse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response()
    }
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
