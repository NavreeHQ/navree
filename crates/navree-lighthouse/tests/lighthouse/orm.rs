//! Lighthouse's tables as navree-orm models: the issues and occurrences it
//! records read back as models, queried with typed columns.

use navree_db::Db;
use navree_lighthouse::entity::{issue, occurrence};
use navree_orm::Model as _;

use crate::support::{builder, flushed};

#[tokio::test]
async fn issues_and_occurrences_are_orm_models() {
    let app = builder().build().await;
    let client = app.client();
    for n in [41, 42] {
        client
            .get(&format!("/api/demo/boom/{n}"))
            .send()
            .await
            .assert_status(500);
    }
    flushed(&app).await;
    let db = app.get::<Db>().await;

    assert_eq!(issue::Model::table_name(), "lighthouse_issues");
    let open = issue::Model::query()
        .where_(issue::Model::STATUS.eq("open"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].count, 2);
    let occurrences = occurrence::Model::query()
        .where_(occurrence::Model::ISSUE_ID.eq(open[0].id))
        .where_(occurrence::Model::STATUS.eq(500))
        .order_by(occurrence::Model::AT)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0].method.as_deref(), Some("GET"));
}
