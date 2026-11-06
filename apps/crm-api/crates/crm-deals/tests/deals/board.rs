//! `GET /api/v1/deals/board`.

use crm_testkit::Role;
use serde_json::{Value, json};

use crate::support::{board, column_titles, company, create_deal, crm, new_deal};

/// The board without the deals: `(stage, label, probability, count,
/// amount_cents)` per column, in order.
fn columns(board: &Value) -> Vec<Value> {
    board["stages"]
        .as_array()
        .expect("stages")
        .iter()
        .map(|column| {
            json!([
                column["stage"],
                column["label"],
                column["probability"],
                column["count"],
                column["amount_cents"],
            ])
        })
        .collect()
}

#[tokio::test]
async fn board_groups_by_stage_with_totals() {
    let crm = crm().await;
    let rep = crm.login_as(Role::Rep1).await;
    let manager = crm.login_as(Role::Manager).await;
    let agra = company(&crm, "Agra", rep.id()).await;
    let rep2 = crm.user_id(Role::Rep2);

    create_deal(&rep, new_deal("A", agra, 100)).await;
    create_deal(&rep, new_deal("B", agra, 250)).await;
    let mut input = new_deal("C", agra, 1_000);
    input["stage"] = json!("proposal");
    input["currency"] = json!("EUR");
    create_deal(&rep, input).await;
    let mut input = new_deal("D", agra, 500);
    input["stage"] = json!("negotiation");
    input["owner_id"] = json!(rep2);
    create_deal(&manager, input).await;
    let mut input = new_deal("E", agra, 700);
    input["stage"] = json!("won");
    create_deal(&rep, input).await;

    let everything = board(&rep).await;
    assert_eq!(
        columns(&everything),
        [
            json!(["lead", "Lead", 10, 2, 350]),
            json!(["qualified", "Qualified", 25, 0, 0]),
            json!(["proposal", "Proposal", 50, 1, 1_000]),
            json!(["negotiation", "Negotiation", 75, 1, 500]),
            json!(["won", "Won", 100, 1, 700]),
            json!(["lost", "Lost", 0, 0, 0]),
        ]
    );
    assert_eq!(column_titles(&everything, "lead"), ["A", "B"]);
    assert_eq!(column_titles(&everything, "proposal"), ["C"]);
    assert_eq!(
        column_titles(&everything, "qualified"),
        Vec::<String>::new()
    );
    let lead = &everything["stages"][0]["deals"][0];
    assert_eq!(lead["__type"], "Deal");
    assert_eq!(lead["company"], json!({ "id": agra, "name": "Agra" }));

    // One owner's board.
    let theirs = rep
        .get("/api/v1/deals/board")
        .query(&json!({ "owner_id": rep2 }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        columns(&theirs),
        [
            json!(["lead", "Lead", 10, 0, 0]),
            json!(["qualified", "Qualified", 25, 0, 0]),
            json!(["proposal", "Proposal", 50, 0, 0]),
            json!(["negotiation", "Negotiation", 75, 1, 500]),
            json!(["won", "Won", 100, 0, 0]),
            json!(["lost", "Lost", 0, 0, 0]),
        ]
    );
    assert_eq!(column_titles(&theirs, "negotiation"), ["D"]);
    crm.client()
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_error(401, None);
    crm.shutdown().await;
}
