//! Seats that follow the workspace's member count.

use crate::support::World;

#[tokio::test]
async fn joining_and_leaving_members_update_the_seat_count() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let alan = world.user("Alan", "alan@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let billing = world.billing().await;
    let sub = world.subscribe(&acme, "pro").await;
    assert_eq!(sub.quantity, 1);

    world.join(acme.id, grace.id, "member").await;
    world.join(acme.id, alan.id, "viewer").await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&acme).await.unwrap().unwrap().quantity,
        3
    );

    world.leave(acme.id, alan.id).await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&acme).await.unwrap().unwrap().quantity,
        2
    );
    // Already in line: nothing to change.
    assert!(billing.sync_seats(acme.id).await.unwrap().is_none());
}

#[tokio::test]
async fn plans_without_seats_and_workspaces_without_subscriptions_are_left_alone() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada.id, "Acme").await;
    let globex = world.workspace(ada.id, "Globex").await;
    let billing = world.billing().await;
    world.subscribe(&acme, "business").await;

    world.join(acme.id, grace.id, "member").await;
    world.join(globex.id, grace.id, "member").await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&acme).await.unwrap().unwrap().quantity,
        1,
        "business is not per seat"
    );
    assert!(billing.subscription(&globex).await.unwrap().is_none());

    // Swapping onto a per-seat plan takes the member count.
    let swapped = billing.swap(&acme, "pro").await.unwrap();
    assert_eq!(swapped.quantity, 2);
    // And off it, back to one.
    let swapped = billing.swap(&acme, "business").await.unwrap();
    assert_eq!(swapped.quantity, 1);
}
