//! Board order: every stage's deals hold the positions `0..n` (spec §9.4).
//!
//! A column is read in board order (`position`, then age) and written back
//! densely, so gaps left by moves and deletes close and ties resolve the
//! same way the board shows them.

use crm_core::Stage;
use crm_core::entities::deal;
use navis::db::Uow;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use uuid::Uuid;

/// One deal of a column: its id and stored position.
pub(crate) type Slot = (Uuid, i32);

/// Where a deal lands in a column and which other deals shift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    /// The deal's position.
    pub(crate) position: i32,
    /// `(id, new position)` of the other deals whose position changes.
    pub(crate) shifts: Vec<Slot>,
}

/// Puts a deal into `column` (the other deals of the target stage, in
/// board order) at the 0-based `index`, or at the end for `None` or an
/// index past the end. The deals from `index` on move down one.
pub(crate) fn insert(column: &[Slot], index: Option<usize>) -> Placement {
    let index = index.map_or(column.len(), |index| index.min(column.len()));
    let shifts = column
        .iter()
        .enumerate()
        .filter_map(|(at, &(id, position))| {
            let wanted = if at < index { at } else { at + 1 };
            let wanted = to_position(wanted);
            (wanted != position).then_some((id, wanted))
        })
        .collect();
    Placement {
        position: to_position(index),
        shifts,
    }
}

/// The changes that number `column` `0..n` in its order.
pub(crate) fn compact(column: &[Slot]) -> Vec<Slot> {
    column
        .iter()
        .enumerate()
        .filter_map(|(at, &(id, position))| {
            let wanted = to_position(at);
            (wanted != position).then_some((id, wanted))
        })
        .collect()
}

fn to_position(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

/// The deals of `stage` except `except`, in board order, locked for the
/// rest of the unit of work where the database supports it.
pub(crate) async fn column(
    uow: &Uow,
    stage: Stage,
    except: Option<Uuid>,
) -> Result<Vec<Slot>, DbErr> {
    let mut select = deal::Entity::find()
        .select_only()
        .column(deal::Column::Id)
        .column(deal::Column::Position)
        .filter(deal::Column::Stage.eq(stage));
    if let Some(id) = except {
        select = select.filter(deal::Column::Id.ne(id));
    }
    if uow.lock_for_update() {
        select = select.lock_exclusive();
    }
    let select = select
        .order_by_asc(deal::Column::Position)
        .order_by_asc(deal::Column::CreatedAt)
        .order_by_asc(deal::Column::Id);
    let tx = uow.tx().await?;
    select.into_tuple().all(&*tx).await
}

/// Writes the `shifts` (bookkeeping, so no change events and no audit).
pub(crate) async fn apply<C: ConnectionTrait>(conn: &C, shifts: &[Slot]) -> Result<(), DbErr> {
    for &(id, position) in shifts {
        deal::Entity::update_many()
            .col_expr(deal::Column::Position, Expr::value(position))
            .filter(deal::Column::Id.eq(id))
            .exec(conn)
            .await?;
    }
    Ok(())
}

/// Numbers the deals of `stage` `0..n` again (after one left it).
pub(crate) async fn close_gap(uow: &Uow, stage: Stage) -> Result<(), DbErr> {
    let shifts = compact(&column(uow, stage, None).await?);
    let tx = uow.tx().await?;
    apply(&*tx, &shifts).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(0xa);
    const B: Uuid = Uuid::from_u128(0xb);
    const C: Uuid = Uuid::from_u128(0xc);

    #[test]
    fn inserting_shifts_the_deals_from_the_index_down() {
        let column = [(A, 0), (B, 1), (C, 2)];
        assert_eq!(
            insert(&column, Some(1)),
            Placement {
                position: 1,
                shifts: vec![(B, 2), (C, 3)],
            }
        );
        assert_eq!(
            insert(&column, Some(0)),
            Placement {
                position: 0,
                shifts: vec![(A, 1), (B, 2), (C, 3)],
            }
        );
    }

    #[test]
    fn the_end_is_the_default_and_the_limit() {
        let column = [(A, 0), (B, 1)];
        let end = Placement {
            position: 2,
            shifts: vec![],
        };
        assert_eq!(insert(&column, None), end);
        assert_eq!(insert(&column, Some(2)), end);
        assert_eq!(insert(&column, Some(99)), end);
        assert_eq!(
            insert(&[], Some(3)),
            Placement {
                position: 0,
                shifts: vec![],
            }
        );
    }

    #[test]
    fn inserting_also_closes_existing_gaps() {
        // Positions 0, 4, 7 (a deal left twice): placed at 1, B and C follow.
        let column = [(A, 0), (B, 4), (C, 7)];
        assert_eq!(
            insert(&column, Some(1)),
            Placement {
                position: 1,
                shifts: vec![(B, 2), (C, 3)],
            }
        );
    }

    #[test]
    fn compacting_numbers_the_column_densely() {
        assert_eq!(compact(&[(A, 1), (B, 2), (C, 2)]), vec![(A, 0), (B, 1)]);
        assert_eq!(compact(&[(A, 0), (B, 1)]), vec![]);
        assert_eq!(compact(&[]), vec![]);
    }
}
