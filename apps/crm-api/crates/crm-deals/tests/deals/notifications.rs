//! `DealAssigned` / `DealWon`: the owner gets a queued database and mail
//! notification.

use crm_testkit::{CrmClient, Role, TestCrm};
use navree::mail::{Mailer, Message};
use serde_json::{Value, json};

use crate::support::{company, create_deal, crm, id_of, move_deal, new_deal};

/// `client`'s notifications, newest first.
async fn notifications(client: &CrmClient) -> Vec<Value> {
    client
        .get("/api/notifications")
        .send()
        .await
        .assert_status(200)
        .json_value()["data"]
        .as_array()
        .expect("data")
        .clone()
}

/// The mails sent so far.
async fn mails(crm: &TestCrm) -> Vec<Message> {
    crm.get::<Mailer>()
        .await
        .array_transport()
        .expect("the array transport")
        .sent()
}

#[tokio::test]
async fn manager_can_reassign_and_owner_notified() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep1.id()).await;
    let id = id_of(&create_deal(&rep1, new_deal("Agra renewal", agra, 100)).await);

    let reassigned = manager
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "owner_id": rep2.id() }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(reassigned["owner_id"], json!(rep2.id()));
    assert_eq!(reassigned["owner"]["name"], Role::Rep2.name());

    // Queued: nothing is delivered until a worker runs.
    assert!(notifications(&rep2).await.is_empty());
    assert!(mails(&crm).await.is_empty());
    let stats = crm.run_queue().await;
    assert_eq!((stats.released, stats.failed), (0, 0), "{stats:?}");
    assert!(stats.succeeded >= 1, "{stats:?}");

    let received = notifications(&rep2).await;
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0]["type"], "deal.assigned");
    assert_eq!(received[0]["read_at"], json!(null));
    let message = format!("{} assigned you “Agra renewal”", Role::Manager.name());
    assert_eq!(
        received[0]["data"],
        json!({
            "title": "Deal assigned to you",
            "message": message,
            "url": format!("/deals/{id}"),
            "deal_id": id,
        })
    );
    let sent = mails(&crm).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].to, [Role::Rep2.email()]);
    assert_eq!(sent[0].subject, "Deal assigned to you: Agra renewal");
    let text = sent[0].text.clone().unwrap_or_default();
    assert!(text.contains(&message), "{text}");
    assert!(text.contains(&format!("/deals/{id}")), "{text}");
    assert!(text.contains(Role::Rep2.name()), "{text}");
    assert!(sent[0].html.is_some());

    // Nobody else hears about it.
    assert!(notifications(&rep1).await.is_empty());
    assert!(notifications(&manager).await.is_empty());
    crm.shutdown().await;
}

#[tokio::test]
async fn winning_notifies_the_owner() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep1.id()).await;
    let id = id_of(&create_deal(&rep1, new_deal("Agra renewal", agra, 100)).await);

    // Won by the manager: the owner (rep1) is told.
    move_deal(&manager, id, json!({ "stage": "won" })).await;
    crm.run_queue().await;
    let received = notifications(&rep1).await;
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0]["type"], "deal.won");
    assert_eq!(
        received[0]["data"],
        json!({
            "title": "Deal won",
            "message": "“Agra renewal” was marked won",
            "url": format!("/deals/{id}"),
            "deal_id": id,
        })
    );
    let sent = mails(&crm).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].to, [Role::Rep1.email()]);
    assert_eq!(sent[0].subject, "Deal won: Agra renewal");
    assert!(notifications(&manager).await.is_empty());

    // Reordering within won is not a second win.
    move_deal(&manager, id, json!({ "stage": "won", "position": 0 })).await;
    crm.run_queue().await;
    assert_eq!(notifications(&rep1).await.len(), 1);
    crm.shutdown().await;
}

#[tokio::test]
async fn own_changes_notify_nobody() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep1.id()).await;

    // A rep's own deal, edited and moved; a manager's deal for themselves.
    let id = id_of(&create_deal(&rep1, new_deal("Agra renewal", agra, 100)).await);
    rep1.patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "title": "Agra renewal 2027", "owner_id": rep1.id() }))
        .send()
        .await
        .assert_status(200);
    move_deal(&rep1, id, json!({ "stage": "proposal" })).await;
    let mut input = new_deal("Gulati", agra, 100);
    input["owner_id"] = json!(manager.id());
    create_deal(&manager, input).await;
    // A manager taking a deal over is not news to them.
    manager
        .patch(&format!("/api/v1/deals/{id}"))
        .json(&json!({ "owner_id": manager.id() }))
        .send()
        .await
        .assert_status(200);

    crm.run_queue().await;
    for client in [&rep1, &rep2, &manager] {
        assert!(notifications(client).await.is_empty(), "{client:?}");
    }
    assert!(mails(&crm).await.is_empty());

    // A deal created for someone else is an assignment.
    let mut input = new_deal("Ilaichi", agra, 100);
    input["owner_id"] = json!(rep2.id());
    create_deal(&manager, input).await;
    crm.run_queue().await;
    let received = notifications(&rep2).await;
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0]["type"], "deal.assigned");
    crm.shutdown().await;
}
