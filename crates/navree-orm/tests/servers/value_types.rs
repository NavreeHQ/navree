//! The query builder over an untyped column (`col("amount")`) binds each
//! value with its own type: an integer one run, a float the next, a
//! decimal after both. Every run must answer as if it were the first, on
//! pooled connections and inside a unit of work (one connection, which
//! keeps what it prepared).

use navree_db::{Db, Executor as _};
use navree_orm::{Value, col, col_as, table};

/// The ids of measures whose amount is above `value`, in order.
async fn above<E: navree_orm::Executor + ?Sized>(db: &E, value: Value) -> Vec<i64> {
    table("measures")
        .where_(col("amount").gt(value))
        .order_by("id")
        .pluck(db, col_as::<i64>("id"))
        .await
        .unwrap()
}

pub async fn untyped_columns_bind_each_value_with_its_type(db: Db) {
    db.execute_script(
        "CREATE TABLE measures (id BIGINT PRIMARY KEY, amount DOUBLE PRECISION NOT NULL)",
    )
    .await
    .unwrap();
    db.execute_script("INSERT INTO measures (id, amount) VALUES (1, 1.0), (2, 2.5), (3, 4.0)")
        .await
        .unwrap();

    // Pooled connections: integers first, then floats.
    for _ in 0..5 {
        assert_eq!(above(&db, Value::I64(1)).await, [2, 3]);
    }
    for run in 0..5 {
        assert_eq!(
            above(&db, Value::F64(3.5)).await,
            [3],
            "`amount > 3.5` after `amount > 1` (run {run})"
        );
    }

    // One connection: a unit of work.
    let uow = db.uow(None, None).await;
    assert_eq!(above(&uow, Value::F64(2.0)).await, [2, 3]);
    assert_eq!(
        above(&uow, Value::I64(3)).await,
        [3],
        "`amount > 3` after `amount > 2.0`"
    );
    assert_eq!(
        above(&uow, Value::F32(2.0)).await,
        [2, 3],
        "`amount > 2.0` (an f32) after an integer"
    );
    assert_eq!(
        above(&uow, Value::Decimal("3.9".parse().unwrap())).await,
        [3],
        "`amount > 3.9` (a decimal) after integers"
    );
}
