//! `deal.updated` `{deal}` on `private-crm.deals` after every change, so
//! open boards refresh (spec §6 real-time, §7).

use std::time::Duration;

use crm_testkit::Role;
use serde_json::json;

use crate::support::{
    CHANNEL, EVENT, Socket, company, create_deal, crm, id_of, move_deal, new_deal, serve,
};

#[tokio::test]
async fn deal_update_broadcasts() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let watcher = crm.login_as(Role::Rep2).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let addr = serve(&crm).await;
    // Another user's board, subscribed the way crm-web subscribes.
    let mut board = Socket::connect(addr).await;
    board.join(&watcher, CHANNEL).await;

    let created = create_deal(&rep, new_deal("Acme renewal", acme, 100)).await;
    let frame = board.expect(EVENT).await;
    assert_eq!(frame.channel.as_deref(), Some(CHANNEL));
    assert_eq!(frame.data, json!({ "deal": created }));
    let id = id_of(&created);

    let updated = rep
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "Acme renewal 2027" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(board.expect(EVENT).await.data, json!({ "deal": updated }));

    let moved = move_deal(&rep, id, json!({ "stage": "proposal", "position": 0 })).await;
    assert_eq!(board.expect(EVENT).await.data, json!({ "deal": moved }));

    // Refused changes broadcast nothing.
    watcher
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "Mine now" }))
        .send()
        .await
        .assert_error(403, None);
    rep.post(&format!("/api/v1/deals/{id}/move"))
        .json(&json!({ "stage": "lost" }))
        .send()
        .await
        .assert_validation_error("lost_reason");
    board.expect_silence(Duration::from_millis(300)).await;

    // A deleted deal is announced with its last state.
    rep.delete(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(204);
    let deleted = board.expect(EVENT).await;
    assert_eq!(deleted.data, json!({ "deal": moved }));

    board.close().await;
    crm.shutdown().await;
}
