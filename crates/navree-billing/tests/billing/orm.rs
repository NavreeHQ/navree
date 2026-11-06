//! The billing tables as navree-orm models: the local copies of customers,
//! subscriptions and their items read back as models.

use navree_billing::entity::{customer, subscription, subscription_item};
use navree_orm::Model as _;

use crate::support::World;

#[tokio::test]
async fn customers_subscriptions_and_items_are_orm_models() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let started = world.subscribe(&agra, "pro").await;
    let db = world.db().await;

    assert_eq!(subscription::Model::table_name(), "subscriptions");
    let rows = subscription::Model::query()
        .where_(subscription::Model::BILLABLE_ID.eq(agra.id))
        .where_(subscription::Model::PLAN.eq("pro"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, started.id);
    assert_eq!(rows[0].provider, "fake");
    assert_eq!(
        customer::Model::query()
            .where_(customer::Model::BILLABLE_ID.eq(agra.id))
            .count(&db)
            .await
            .unwrap(),
        1
    );
    assert!(
        subscription_item::Model::query()
            .where_(subscription_item::Model::SUBSCRIPTION_ID.eq(started.id))
            .exists(&db)
            .await
            .unwrap()
    );
}
