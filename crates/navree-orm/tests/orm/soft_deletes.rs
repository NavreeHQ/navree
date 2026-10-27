//! Soft-deleting models: deleting sets the deleted-at column, queries skip
//! trashed rows until `with_trashed`.

use navree_orm::Model;

use crate::models::Deal;
use crate::support::{count_rows, sqlite};

#[tokio::test]
async fn deleting_trashes_the_row() {
    let t = sqlite().await;
    let mut gone = Deal::create(&t.db, Deal::new("Gone", 1)).await.unwrap();
    Deal::create(&t.db, Deal::new("Kept", 2)).await.unwrap();
    assert!(!gone.trashed());
    assert!(gone.delete(&t.db).await.unwrap());
    assert!(gone.trashed());
    assert!(gone.exists(), "a trashed record is still stored");
    assert!(gone.is_clean());
    assert_eq!(gone.updated_at, gone.deleted_at.unwrap());
    assert_eq!(count_rows(&t.db, "deals").await, 2);

    assert_eq!(Deal::query().count(&t.db).await.unwrap(), 1);
    assert!(Deal::find(&t.db, gone.id).await.unwrap().is_none());
    assert_eq!(Deal::query().with_trashed().count(&t.db).await.unwrap(), 2);
    let trashed = Deal::query()
        .with_trashed()
        .find(&t.db, gone.id)
        .await
        .unwrap()
        .unwrap();
    assert!(trashed.trashed());

    // Refresh and fresh see trashed rows (they ignore global scopes).
    assert!(gone.fresh(&t.db).await.unwrap().is_some());
    gone.refresh(&t.db).await.unwrap();
}

#[tokio::test]
async fn the_scope_keeps_the_callers_or_conditions_together() {
    let t = sqlite().await;
    let mut a = Deal::create(&t.db, Deal::new("A", 1)).await.unwrap();
    Deal::create(&t.db, Deal::new("B", 2)).await.unwrap();
    a.delete(&t.db).await.unwrap();
    // `title = A OR title = B` stays grouped: the trashed A is not found.
    let found = Deal::query()
        .where_(Deal::TITLE.eq("A"))
        .or_where(Deal::TITLE.eq("B"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(found.map(|deal| deal.title.clone()), ["B"]);
}

#[tokio::test]
async fn mass_deletes_trash_every_match() {
    let t = sqlite().await;
    for (title, cents) in [("A", 1), ("B", 2), ("C", 300)] {
        Deal::create(&t.db, Deal::new(title, cents)).await.unwrap();
    }
    let trashed = Deal::query()
        .where_(Deal::AMOUNT_CENTS.lt(100))
        .delete(&t.db)
        .await
        .unwrap();
    assert_eq!(trashed, 2);
    assert_eq!(Deal::query().count(&t.db).await.unwrap(), 1);
    assert_eq!(count_rows(&t.db, "deals").await, 3);
    // Trashed rows are not matched again.
    assert_eq!(
        Deal::query()
            .where_(Deal::AMOUNT_CENTS.lt(100))
            .delete(&t.db)
            .await
            .unwrap(),
        0
    );
}
