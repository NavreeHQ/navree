//! What staff can do to an order or a product, and when the shop says no. Every
//! rule here runs on the server: a button the page shows enabled is only a hint,
//! and a page left open while the order moved on gets the same answer as anyone.

use navree::orm::Executor;
use navree::orm::Model as _;
use navree::prelude::BoxError;

use crate::data::{Event, Fulfilment, Kind, Level, Line, Order, Payment, Product, STAFF, Status};
use crate::views::money;

/// A confirmation goes out at most this often, in seconds.
pub const RESEND_EVERY: i64 = 60;
/// The most units one restock adds.
pub const RESTOCK_MAX: i64 = 500;

/// Why the shop refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Paid for by bank transfer that hasn't arrived: nothing to give back yet.
    NotPaid,
    /// Everything paid has gone back already.
    NothingLeft,
    /// An amount of zero or less.
    NotPositive,
    /// More than is left to refund; the most that is.
    TooMuch(i64),
    /// It has left the warehouse.
    Shipped,
    /// It was cancelled already.
    Cancelled,
    /// A confirmation went out this many seconds ago.
    SentLately(i64),
    /// A restock of a number of units outside 1 to [`RESTOCK_MAX`].
    BadQuantity,
    /// No such order or product.
    Missing,
}

impl Refusal {
    /// What the toast says.
    pub fn message(&self, number: &str) -> String {
        match self {
            Self::NotPaid => format!("{number} isn't paid yet, so there's nothing to refund."),
            Self::NothingLeft => format!("{number} is refunded in full already."),
            Self::NotPositive => "Enter an amount above zero.".to_owned(),
            Self::TooMuch(most) => {
                format!("At most {} is left to refund on {number}.", money(*most))
            }
            Self::Shipped => {
                format!("{number} has shipped, so it can't be cancelled. Refund it instead.")
            }
            Self::Cancelled => format!("{number} is cancelled already."),
            Self::SentLately(seconds) => format!(
                "A confirmation for {number} went out {seconds} s ago. Try again in a minute."
            ),
            Self::BadQuantity => format!("Restock between 1 and {RESTOCK_MAX} units."),
            Self::Missing => "That isn't in the shop any more.".to_owned(),
        }
    }
}

/// How much is left to refund, or why nothing can be.
pub fn refundable(order: &Order) -> Result<i64, Refusal> {
    match Payment::parse(&order.payment) {
        Payment::Pending => Err(Refusal::NotPaid),
        Payment::Refunded => Err(Refusal::NothingLeft),
        Payment::Paid | Payment::PartiallyRefunded => {
            let left = order.total - order.refunded;
            if left > 0 {
                Ok(left)
            } else {
                Err(Refusal::NothingLeft)
            }
        }
    }
}

/// Whether `amount` cents can be refunded now.
pub fn check_refund(order: &Order, amount: i64) -> Result<(), Refusal> {
    let left = refundable(order)?;
    if amount <= 0 {
        return Err(Refusal::NotPositive);
    }
    if amount > left {
        return Err(Refusal::TooMuch(left));
    }
    Ok(())
}

/// Whether the order can be cancelled now: not once it has left the warehouse.
pub fn check_cancel(order: &Order) -> Result<(), Refusal> {
    if Status::parse(&order.status) == Status::Cancelled {
        return Err(Refusal::Cancelled);
    }
    if Fulfilment::parse(&order.fulfilment) >= Fulfilment::Shipped {
        return Err(Refusal::Shipped);
    }
    Ok(())
}

/// Whether the confirmation can go out again at `now`.
pub fn check_resend(order: &Order, now: i64) -> Result<(), Refusal> {
    if Status::parse(&order.status) == Status::Cancelled {
        return Err(Refusal::Cancelled);
    }
    let ago = now - order.confirmed_at;
    if ago < RESEND_EVERY {
        return Err(Refusal::SentLately(ago.max(0)));
    }
    Ok(())
}

/// Whether `quantity` units can be added in one restock.
pub fn check_restock(quantity: i64) -> Result<(), Refusal> {
    if (1..=RESTOCK_MAX).contains(&quantity) {
        Ok(())
    } else {
        Err(Refusal::BadQuantity)
    }
}

/// What an action changed, for the toast.
#[derive(Clone, Debug)]
pub struct Done {
    pub order: Order,
    pub text: String,
}

async fn load<E: Executor + ?Sized>(db: &E, id: i64) -> Result<Option<Order>, BoxError> {
    Ok(Order::query()
        .find(db, id)
        .await?
        .map(|record| record.into_inner()))
}

/// Writes an event and gives back its id.
pub async fn record<E: Executor + ?Sized>(
    db: &E,
    next_id: i64,
    event: Event,
) -> Result<Event, BoxError> {
    let event = Event {
        id: next_id,
        ..event
    };
    Event::query().insert(db, [event.clone()]).await?;
    Ok(event)
}

/// Refunds `amount` cents of order `id` for `reason`. The write only lands if
/// nobody refunded in between, so two refunds at once can't give back more than
/// was paid.
pub async fn refund<E: Executor + ?Sized>(
    db: &E,
    id: i64,
    amount: i64,
    reason: &str,
    now: i64,
    next_event: impl FnOnce() -> i64,
) -> Result<Result<Done, (Option<Order>, Refusal)>, BoxError> {
    let Some(order) = load(db, id).await? else {
        return Ok(Err((None, Refusal::Missing)));
    };
    if let Err(refusal) = check_refund(&order, amount) {
        return Ok(Err((Some(order), refusal)));
    }
    let refunded = order.refunded + amount;
    let payment = if refunded >= order.total {
        Payment::Refunded
    } else {
        Payment::PartiallyRefunded
    };
    let changed = Order::query()
        .where_(Order::ID.eq(id))
        .where_(Order::REFUNDED.eq(order.refunded))
        .update(
            db,
            [
                Order::REFUNDED.set(refunded),
                Order::PAYMENT.set(payment.key()),
            ],
        )
        .await?;
    if changed != 1 {
        let order = load(db, id).await?;
        let refusal = order
            .as_ref()
            .map_or(Refusal::Missing, |order| match refundable(order) {
                Ok(left) => Refusal::TooMuch(left),
                Err(refusal) => refusal,
            });
        return Ok(Err((order, refusal)));
    }
    record(
        db,
        next_event(),
        Event {
            kind: Kind::Refunded.key().to_owned(),
            at: now,
            order_id: id,
            amount,
            actor: STAFF.to_owned(),
            detail: reason.to_owned(),
            ..Event::default()
        },
    )
    .await?;
    let order = load(db, id).await?.unwrap_or(order);
    let text = if payment == Payment::Refunded {
        format!(
            "{} refunded in full: {} back to the customer.",
            order.number,
            money(amount)
        )
    } else {
        format!(
            "{} back to the customer on {}. {} is left to refund.",
            money(amount),
            order.number,
            money(order.total - order.refunded)
        )
    };
    Ok(Ok(Done { order, text }))
}

/// Cancels order `id`: anything paid goes back, and its units go back on the
/// shelf.
pub async fn cancel<E: Executor + ?Sized>(
    db: &E,
    id: i64,
    now: i64,
    mut next_event: impl FnMut() -> i64,
) -> Result<Result<Done, (Option<Order>, Refusal)>, BoxError> {
    let Some(order) = load(db, id).await? else {
        return Ok(Err((None, Refusal::Missing)));
    };
    if let Err(refusal) = check_cancel(&order) {
        return Ok(Err((Some(order), refusal)));
    }
    let back = refundable(&order).unwrap_or(0);
    let payment = if back > 0 {
        Payment::Refunded
    } else {
        Payment::parse(&order.payment)
    };
    let changed = Order::query()
        .where_(Order::ID.eq(id))
        .where_(Order::STATUS.ne(Status::Cancelled.key()))
        .where_(Order::FULFILMENT.is_in([Fulfilment::Unfulfilled.key(), Fulfilment::Packed.key()]))
        .update(
            db,
            [
                Order::STATUS.set(Status::Cancelled.key()),
                Order::PAYMENT.set(payment.key()),
                Order::REFUNDED.set(order.refunded + back),
            ],
        )
        .await?;
    if changed != 1 {
        let order = load(db, id).await?;
        let refusal = order
            .as_ref()
            .and_then(|order| check_cancel(order).err())
            .unwrap_or(Refusal::Missing);
        return Ok(Err((order, refusal)));
    }
    record(
        db,
        next_event(),
        Event {
            kind: Kind::Cancelled.key().to_owned(),
            at: now,
            order_id: id,
            actor: STAFF.to_owned(),
            detail: "Cancelled by the shop".to_owned(),
            ..Event::default()
        },
    )
    .await?;
    if back > 0 {
        record(
            db,
            next_event(),
            Event {
                kind: Kind::Refunded.key().to_owned(),
                at: now,
                order_id: id,
                amount: back,
                actor: STAFF.to_owned(),
                detail: "Cancelled".to_owned(),
                ..Event::default()
            },
        )
        .await?;
    }
    // The units go back on the shelf.
    let lines: Vec<Line> = Line::query()
        .where_(Line::ORDER_ID.eq(id))
        .get(db)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    for line in &lines {
        add_stock(db, line.product_id, line.quantity).await?;
    }
    let units: i64 = lines.iter().map(|line| line.quantity).sum();
    let order = load(db, id).await?.unwrap_or(order);
    let text = if back > 0 {
        format!(
            "{} is cancelled: {} back to the customer, and {units} {} back in stock.",
            order.number,
            money(back),
            if units == 1 { "unit" } else { "units" }
        )
    } else {
        format!(
            "{} is cancelled, and {units} {} back in stock.",
            order.number,
            if units == 1 { "unit" } else { "units" }
        )
    };
    Ok(Ok(Done { order, text }))
}

/// Sends order `id`'s confirmation again.
pub async fn resend<E: Executor + ?Sized>(
    db: &E,
    id: i64,
    now: i64,
    next_event: impl FnOnce() -> i64,
) -> Result<Result<Done, (Option<Order>, Refusal)>, BoxError> {
    let Some(order) = load(db, id).await? else {
        return Ok(Err((None, Refusal::Missing)));
    };
    if let Err(refusal) = check_resend(&order, now) {
        return Ok(Err((Some(order), refusal)));
    }
    let changed = Order::query()
        .where_(Order::ID.eq(id))
        .where_(Order::CONFIRMED_AT.lte(now - RESEND_EVERY))
        .update(db, [Order::CONFIRMED_AT.set(now)])
        .await?;
    if changed != 1 {
        let order = load(db, id).await?;
        let refusal = order
            .as_ref()
            .and_then(|order| check_resend(order, now).err())
            .unwrap_or(Refusal::Missing);
        return Ok(Err((order, refusal)));
    }
    record(
        db,
        next_event(),
        Event {
            kind: Kind::Resent.key().to_owned(),
            at: now,
            order_id: id,
            actor: STAFF.to_owned(),
            detail: order.email.clone(),
            ..Event::default()
        },
    )
    .await?;
    let order = load(db, id).await?.unwrap_or(order);
    let text = format!(
        "The confirmation of {} is on its way to {}.",
        order.number, order.email
    );
    Ok(Ok(Done { order, text }))
}

/// Adds `units` to a product's stock and keeps its level with it. Returns the
/// product as it is now.
pub async fn add_stock<E: Executor + ?Sized>(
    db: &E,
    id: i64,
    units: i64,
) -> Result<Option<Product>, BoxError> {
    let Some(product) = Product::query().find(db, id).await? else {
        return Ok(None);
    };
    let product = product.into_inner();
    let stock = (product.stock + units).max(0);
    let level = Level::of(stock, product.reorder_at);
    Product::query()
        .where_(Product::ID.eq(id))
        .update(
            db,
            [Product::STOCK.set(stock), Product::LEVEL.set(level.key())],
        )
        .await?;
    Ok(Some(Product {
        stock,
        level: level.key().to_owned(),
        ..product
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(payment: Payment, fulfilment: Fulfilment, total: i64, refunded: i64) -> Order {
        Order {
            number: "FH-20481".to_owned(),
            status: Status::Open.key().to_owned(),
            payment: payment.key().to_owned(),
            fulfilment: fulfilment.key().to_owned(),
            total,
            refunded,
            confirmed_at: 1_000,
            ..Order::default()
        }
    }

    #[test]
    fn a_refund_gives_back_at_most_what_is_left() {
        let paid = order(Payment::Paid, Fulfilment::Delivered, 18_400, 0);
        assert_eq!(refundable(&paid), Ok(18_400));
        assert_eq!(check_refund(&paid, 4_000), Ok(()));
        assert_eq!(check_refund(&paid, 18_400), Ok(()));
        assert_eq!(check_refund(&paid, 18_401), Err(Refusal::TooMuch(18_400)));
        assert_eq!(check_refund(&paid, 0), Err(Refusal::NotPositive));
        assert_eq!(check_refund(&paid, -5), Err(Refusal::NotPositive));
        let partly = order(
            Payment::PartiallyRefunded,
            Fulfilment::Delivered,
            18_400,
            4_000,
        );
        assert_eq!(check_refund(&partly, 14_401), Err(Refusal::TooMuch(14_400)));
        let done = order(Payment::Refunded, Fulfilment::Delivered, 18_400, 18_400);
        assert_eq!(check_refund(&done, 100), Err(Refusal::NothingLeft));
        let pending = order(Payment::Pending, Fulfilment::Unfulfilled, 18_400, 0);
        assert_eq!(check_refund(&pending, 100), Err(Refusal::NotPaid));
    }

    #[test]
    fn nothing_that_has_shipped_can_be_cancelled() {
        for step in [Fulfilment::Unfulfilled, Fulfilment::Packed] {
            assert_eq!(check_cancel(&order(Payment::Paid, step, 100, 0)), Ok(()));
        }
        for step in [Fulfilment::Shipped, Fulfilment::Delivered] {
            assert_eq!(
                check_cancel(&order(Payment::Paid, step, 100, 0)),
                Err(Refusal::Shipped)
            );
        }
        let mut cancelled = order(Payment::Refunded, Fulfilment::Unfulfilled, 100, 100);
        cancelled.status = Status::Cancelled.key().to_owned();
        assert_eq!(check_cancel(&cancelled), Err(Refusal::Cancelled));
    }

    #[test]
    fn a_confirmation_goes_out_once_a_minute_at_most() {
        let sent = order(Payment::Paid, Fulfilment::Packed, 100, 0);
        assert_eq!(check_resend(&sent, 1_030), Err(Refusal::SentLately(30)));
        assert_eq!(check_resend(&sent, 1_060), Ok(()));
        assert_eq!(check_restock(0), Err(Refusal::BadQuantity));
        assert_eq!(check_restock(501), Err(Refusal::BadQuantity));
        assert_eq!(check_restock(40), Ok(()));
    }

    #[test]
    fn the_words_name_the_order() {
        assert_eq!(
            Refusal::TooMuch(14_400).message("FH-20481"),
            "At most €144.00 is left to refund on FH-20481."
        );
        assert!(
            Refusal::Shipped
                .message("FH-20481")
                .contains("Refund it instead")
        );
    }
}
