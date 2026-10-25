//! `POST /api/v1/deals/{id}/move` and stage changes through `PATCH`.

use std::time::Duration;

use crm_testkit::Role;
use serde_json::json;
use uuid::Uuid;

use crate::support::{
    board, column_positions, column_titles, company, create_deal, crm, id_of, move_deal, new_deal,
};

#[tokio::test]
async fn move_to_lost_requires_reason() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let deal = create_deal(&rep, new_deal("Acme renewal", acme, 100)).await;
    let id = id_of(&deal);
    let path = format!("/api/v1/deals/{id}/move");

    for body in [
        json!({ "stage": "lost" }),
        json!({ "stage": "lost", "lost_reason": "   " }),
        json!({ "stage": "lost", "lost_reason": null }),
    ] {
        let response = rep
            .post(&path)
            .json(&body)
            .send()
            .await
            .assert_validation_error("lost_reason");
        assert_eq!(
            response.json_value()["errors"]["lost_reason"],
            json!(["lost_reason should not be empty"])
        );
    }
    // Nothing changed.
    let unchanged = rep
        .get(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(unchanged, deal);

    crm.clock().advance(Duration::from_secs(10 * 60));
    let lost = move_deal(
        &rep,
        id,
        json!({ "stage": "lost", "lost_reason": "Chose a competitor" }),
    )
    .await;
    assert_eq!(lost["stage"], "lost");
    assert_eq!(lost["probability"], 0);
    assert_eq!(lost["lost_reason"], "Chose a competitor");
    assert_eq!(lost["closed_at"], "2026-10-26T10:10:00Z");
    crm.shutdown().await;
}

#[tokio::test]
async fn won_sets_closed_at() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let id = id_of(&create_deal(&rep, new_deal("Acme renewal", acme, 100)).await);

    crm.clock().advance(Duration::from_secs(10 * 60));
    let won = move_deal(&rep, id, json!({ "stage": "won" })).await;
    assert_eq!(won["stage"], "won");
    assert_eq!(won["probability"], 100);
    assert_eq!(won["closed_at"], "2026-10-26T10:10:00Z");
    assert_eq!(won["lost_reason"], json!(null));
    assert_eq!(won["updated_at"], "2026-10-26T10:10:00Z");

    // Reordering within won keeps the closing date.
    crm.clock().advance(Duration::from_secs(60));
    let still_won = move_deal(&rep, id, json!({ "stage": "won", "position": 0 })).await;
    assert_eq!(still_won["closed_at"], "2026-10-26T10:10:00Z");
    crm.shutdown().await;
}

#[tokio::test]
async fn moving_back_reopens_the_deal() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let id = id_of(&create_deal(&rep, new_deal("Acme renewal", acme, 100)).await);

    move_deal(
        &rep,
        id,
        json!({ "stage": "lost", "lost_reason": "No budget" }),
    )
    .await;
    let reopened = move_deal(&rep, id, json!({ "stage": "negotiation" })).await;
    assert_eq!(reopened["stage"], "negotiation");
    assert_eq!(reopened["probability"], 75);
    assert_eq!(reopened["closed_at"], json!(null));
    assert_eq!(reopened["lost_reason"], json!(null));

    move_deal(&rep, id, json!({ "stage": "won" })).await;
    let reopened = move_deal(&rep, id, json!({ "stage": "lead" })).await;
    assert_eq!(reopened["closed_at"], json!(null));
    crm.shutdown().await;
}

#[tokio::test]
async fn move_reorders_within_the_stage() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let a = id_of(&create_deal(&rep, new_deal("A", acme, 100)).await);
    create_deal(&rep, new_deal("B", acme, 100)).await;
    let c = id_of(&create_deal(&rep, new_deal("C", acme, 100)).await);

    let moved = move_deal(&rep, c, json!({ "stage": "lead", "position": 0 })).await;
    assert_eq!(moved["position"], 0);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "lead"), ["C", "A", "B"]);
    assert_eq!(column_positions(&after, "lead"), [0, 1, 2]);

    move_deal(&rep, a, json!({ "stage": "lead", "position": 2 })).await;
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "lead"), ["C", "B", "A"]);
    assert_eq!(column_positions(&after, "lead"), [0, 1, 2]);

    // Past the end (or no position) means the end.
    let moved = move_deal(&rep, c, json!({ "stage": "lead", "position": 99 })).await;
    assert_eq!(moved["position"], 2);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "lead"), ["B", "A", "C"]);
    assert_eq!(column_positions(&after, "lead"), [0, 1, 2]);
    crm.shutdown().await;
}

#[tokio::test]
async fn move_between_stages_shifts_both_columns() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    create_deal(&rep, new_deal("A", acme, 100)).await;
    let b = id_of(&create_deal(&rep, new_deal("B", acme, 100)).await);
    let c = id_of(&create_deal(&rep, new_deal("C", acme, 100)).await);
    for title in ["D", "E"] {
        let mut input = new_deal(title, acme, 100);
        input["stage"] = json!("proposal");
        create_deal(&rep, input).await;
    }

    let moved = move_deal(&rep, b, json!({ "stage": "proposal", "position": 1 })).await;
    assert_eq!(moved["stage"], "proposal");
    assert_eq!(moved["probability"], 50);
    assert_eq!(moved["position"], 1);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "proposal"), ["D", "B", "E"]);
    assert_eq!(column_positions(&after, "proposal"), [0, 1, 2]);
    assert_eq!(column_titles(&after, "lead"), ["A", "C"]);
    assert_eq!(column_positions(&after, "lead"), [0, 1]);

    // Without a position: the end of the column.
    let moved = move_deal(&rep, c, json!({ "stage": "proposal" })).await;
    assert_eq!(moved["position"], 3);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "proposal"), ["D", "B", "E", "C"]);
    assert_eq!(column_titles(&after, "lead"), ["A"]);
    assert_eq!(column_positions(&after, "lead"), [0]);
    crm.shutdown().await;
}

#[tokio::test]
async fn move_validates_its_input() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let id = id_of(&create_deal(&rep, new_deal("A", acme, 100)).await);
    let path = format!("/api/v1/deals/{id}/move");

    rep.post(&path)
        .json(&json!({}))
        .send()
        .await
        .assert_validation_error("stage");
    rep.post(&path)
        .json(&json!({ "stage": "closed" }))
        .send()
        .await
        .assert_validation_error("stage");
    rep.post(&path)
        .json(&json!({ "stage": "qualified", "position": -1 }))
        .send()
        .await
        .assert_validation_error("position");
    rep.post(&format!("/api/v1/deals/{}/move", Uuid::now_v7()))
        .json(&json!({ "stage": "qualified" }))
        .send()
        .await
        .assert_error(404, None);
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_move_others_deal() {
    let crm = crm().await;
    let rep1 = crm.login_as(Role::Rep1).await;
    let rep2 = crm.login_as(Role::Rep2).await;
    let manager = crm.login_as(Role::Manager).await;
    let acme = company(&crm, "Acme", rep1.id()).await;
    let deal = create_deal(&rep1, new_deal("Acme renewal", acme, 100)).await;
    let id = id_of(&deal);

    rep2.post(&format!("/api/v1/deals/{id}/move"))
        .json(&json!({ "stage": "qualified" }))
        .send()
        .await
        .assert_error(403, None);
    let unchanged = rep1
        .get(&format!("/api/v1/deals/{id}"))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(unchanged, deal);

    let moved = move_deal(&manager, id, json!({ "stage": "qualified" })).await;
    assert_eq!(moved["stage"], "qualified");
    let moved = move_deal(&rep1, id, json!({ "stage": "proposal" })).await;
    assert_eq!(moved["stage"], "proposal");
    crm.shutdown().await;
}

#[tokio::test]
async fn rep_cannot_delete_won_deal() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let admin = crm.login_as(Role::Admin).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let won = id_of(&create_deal(&rep, new_deal("Won", acme, 100)).await);
    move_deal(&rep, won, json!({ "stage": "won" })).await;
    let lost = id_of(&create_deal(&rep, new_deal("Lost", acme, 100)).await);
    move_deal(
        &rep,
        lost,
        json!({ "stage": "lost", "lost_reason": "No budget" }),
    )
    .await;
    let open = id_of(&create_deal(&rep, new_deal("Open", acme, 100)).await);

    for id in [won, lost] {
        // Not even their own, and not a manager either (spec §9.2).
        rep.delete(&format!("/api/v1/deals/{id}"))
            .send()
            .await
            .assert_error(403, None);
        manager
            .delete(&format!("/api/v1/deals/{id}"))
            .send()
            .await
            .assert_error(403, None);
    }
    rep.delete(&format!("/api/v1/deals/{open}"))
        .send()
        .await
        .assert_status(204);
    admin
        .delete(&format!("/api/v1/deals/{won}"))
        .send()
        .await
        .assert_status(204);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "won"), Vec::<String>::new());
    assert_eq!(column_titles(&after, "lost"), ["Lost"]);
    crm.shutdown().await;
}

#[tokio::test]
async fn patching_the_stage_follows_the_move_rules() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    let a = id_of(&create_deal(&rep, new_deal("A", acme, 100)).await);
    create_deal(&rep, new_deal("B", acme, 100)).await;
    let mut input = new_deal("W", acme, 100);
    input["stage"] = json!("won");
    create_deal(&rep, input).await;
    let path = format!("/api/v1/deals/{a}");

    rep.patch(&path)
        .json(&json!({ "stage": "lost" }))
        .send()
        .await
        .assert_validation_error("lost_reason");

    crm.clock().advance(Duration::from_secs(60));
    let won = rep
        .patch(&path)
        .json(&json!({ "stage": "won", "title": "A (signed)" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(won["stage"], "won");
    assert_eq!(won["closed_at"], "2026-10-26T10:01:00Z");
    assert_eq!(won["position"], 1);
    let after = board(&rep).await;
    assert_eq!(column_titles(&after, "won"), ["W", "A (signed)"]);
    assert_eq!(column_titles(&after, "lead"), ["B"]);
    assert_eq!(column_positions(&after, "lead"), [0]);

    let lost = rep
        .patch(&path)
        .json(&json!({ "stage": "lost", "lost_reason": "Cancelled" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(lost["lost_reason"], "Cancelled");
    let reopened = rep
        .patch(&path)
        .json(&json!({ "stage": "qualified" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(reopened["closed_at"], json!(null));
    assert_eq!(reopened["lost_reason"], json!(null));
    crm.shutdown().await;
}

#[tokio::test]
async fn moves_audit_the_moved_deal_only() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let admin = crm.login_as(Role::Admin).await;
    let acme = company(&crm, "Acme", rep.id()).await;
    create_deal(&rep, new_deal("A", acme, 100)).await;
    create_deal(&rep, new_deal("B", acme, 100)).await;
    let c = id_of(&create_deal(&rep, new_deal("C", acme, 100)).await);

    // C jumps to the top: A and B shift down, but only C's move is news.
    move_deal(&rep, c, json!({ "stage": "qualified" })).await;
    move_deal(&rep, c, json!({ "stage": "lead", "position": 0 })).await;
    let audit = admin
        .get("/api/audit")
        .query(&json!({ "subject_type": "Deal", "action": "deal.updated" }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    let entries = audit["data"].as_array().expect("data");
    let subjects: Vec<&str> = entries
        .iter()
        .map(|entry| entry["subject_id"].as_str().expect("a subject id"))
        .collect();
    let c = c.to_string();
    assert_eq!(subjects, [c.as_str(), c.as_str()], "{audit}");
    assert_eq!(entries[0]["actor_id"], json!(rep.id()));
    assert_eq!(entries[0]["new_values"]["stage"], "lead");
    assert_eq!(entries[0]["old_values"]["stage"], "qualified");
    crm.shutdown().await;
}
