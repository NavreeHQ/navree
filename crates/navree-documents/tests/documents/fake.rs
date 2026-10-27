//! `DocumentsFake`: rendering without engines, recording, assertions and
//! failures.

use navree_documents::{DocumentsError, DocumentsFake, EngineKind, Render, Templates};
use serde_json::json;

#[tokio::test]
async fn the_fake_renders_any_name_and_records_it() {
    let fake = DocumentsFake::new();
    let documents = fake.documents();
    let pdf = documents
        .pdf(
            "certificates/classic",
            json!({"name": "Ada", "score": 98, "passed": true}),
        )
        .await
        .unwrap();
    assert_eq!(
        pdf.text().unwrap(),
        "certificates/classic\nname: Ada\npassed: true\nscore: 98"
    );
    documents
        .render(&Render::html("<p>x</p>").locale("ar").title("Inline"))
        .await
        .unwrap();
    assert_eq!(
        fake.rendered_names(),
        vec!["certificates/classic", "inline"]
    );
    fake.assert_rendered("certificates/classic");
    fake.assert_rendered("certificates/classic.html");
    fake.assert_rendered_with("certificates/classic", |render| render.data["score"] == 98);
    fake.assert_rendered_with("inline", |render| render.locale.as_deref() == Some("ar"));
    fake.assert_not_rendered("reports/audit");
    let rendered = fake.rendered();
    assert_eq!(rendered[1].title.as_deref(), Some("Inline"));
}

#[test]
#[should_panic(expected = "the document `reports/audit` was not rendered")]
fn assert_rendered_fails_with_what_was_rendered() {
    DocumentsFake::new().assert_rendered("reports/audit");
}

#[tokio::test]
#[should_panic(expected = "was rendered, but never as expected")]
async fn assert_rendered_with_fails_when_no_render_matches() {
    let fake = DocumentsFake::new();
    fake.documents()
        .pdf("certificates/classic", json!({"name": "Ada"}))
        .await
        .unwrap();
    fake.assert_rendered_with("certificates/classic", |render| {
        render.data["name"] == "Grace"
    });
}

#[tokio::test]
#[should_panic(expected = "documents were rendered")]
async fn assert_nothing_rendered_fails_after_a_render() {
    let fake = DocumentsFake::new();
    fake.documents().pdf("x", json!({})).await.unwrap();
    fake.assert_nothing_rendered();
}

#[tokio::test]
async fn a_fake_with_templates_knows_which_exist() {
    let fake = DocumentsFake::with_templates(Templates::from_files([
        ("certificates/classic.html", "x"),
        ("invoices/show.html", "x"),
    ]));
    let documents = fake.documents();
    documents
        .pdf("certificates/classic", json!({}))
        .await
        .unwrap();
    documents.pdf("invoices/show", json!({})).await.unwrap();
    assert_eq!(
        documents
            .engine_for(&Render::template("invoices/show"))
            .unwrap(),
        EngineKind::Native
    );
    match documents.pdf("reports/audit", json!({})).await {
        Err(DocumentsError::TemplateNotFound { tried, .. }) => {
            assert_eq!(tried, vec!["reports/audit.html"]);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn failures_can_be_arranged() {
    let fake = DocumentsFake::new().fail("reports/audit", "the audit template is broken");
    let documents = fake.documents();
    match documents.pdf("reports/audit", json!({})).await {
        Err(DocumentsError::Template { file, message, .. }) => {
            assert_eq!(file, "reports/audit");
            assert_eq!(message, "the audit template is broken");
        }
        other => panic!("{other:?}"),
    }
    documents.pdf("reports/other", json!({})).await.unwrap();
    fake.assert_rendered("reports/audit");
}

#[tokio::test]
async fn queued_renders_are_recorded_not_rendered() {
    let fake = DocumentsFake::new();
    let documents = fake.documents();
    let queued = documents
        .queue(
            Render::template("reports/audit").data(json!({"quarter": "Q3"})),
            "exports/audit-q3.pdf",
        )
        .await
        .unwrap();
    assert_eq!(queued.path, "exports/audit-q3.pdf");
    assert_eq!(queued.job_id, None);
    fake.assert_queued("reports/audit");
    fake.assert_nothing_rendered();
    let recorded = fake.queued();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].document, queued);
    assert_eq!(recorded[0].render.data["quarter"], "Q3");
    assert_eq!(
        documents.status(&queued).await.unwrap(),
        navree_documents::DocumentStatus::Pending
    );
}
