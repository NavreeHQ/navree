//! Seats that follow the workspace's member count.

use crate::support::World;

#[tokio::test]
async fn joining_and_leaving_members_update_the_seat_count() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let alan = world.user("Arun", "arun@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let billing = world.billing().await;
    let sub = world.subscribe(&agra, "pro").await;
    assert_eq!(sub.quantity, 1);

    world.join(agra.id, grace.id, "member").await;
    world.join(agra.id, alan.id, "viewer").await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&agra).await.unwrap().unwrap().quantity,
        3
    );

    world.leave(agra.id, alan.id).await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&agra).await.unwrap().unwrap().quantity,
        2
    );
    // Already in line: nothing to change.
    assert!(billing.sync_seats(agra.id).await.unwrap().is_none());
}

#[tokio::test]
async fn plans_without_seats_and_workspaces_without_subscriptions_are_left_alone() {
    let world = World::new().await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada.id, "Agra").await;
    let gulati = world.workspace(ada.id, "Gulati").await;
    let billing = world.billing().await;
    world.subscribe(&agra, "business").await;

    world.join(agra.id, grace.id, "member").await;
    world.join(gulati.id, grace.id, "member").await;
    world.run_queue().await;
    assert_eq!(
        billing.subscription(&agra).await.unwrap().unwrap().quantity,
        1,
        "business is not per seat"
    );
    assert!(billing.subscription(&gulati).await.unwrap().is_none());

    // Swapping onto a per-seat plan takes the member count.
    let swapped = billing.swap(&agra, "pro").await.unwrap();
    assert_eq!(swapped.quantity, 2);
    // And off it, back to one.
    let swapped = billing.swap(&agra, "business").await.unwrap();
    assert_eq!(swapped.quantity, 1);
}
