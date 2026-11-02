//! What an export answers while it runs, and what it leaves in the audit log:
//! access read again on every chunk, a time limit, the file a spreadsheet opens.

use std::time::Duration;

use futures::StreamExt;
use navree_db::Db;
use navree_orm::Model;
use navree_testing::{TestApp, TestClient};
use serde_json::Value;
use uuid::Uuid;

use crate::support::*;

fn deal_row(i: usize) -> Deal {
    Deal {
        id: Uuid::now_v7(),
        title: format!("R{i:05}"),
        amount_cents: 100,
        currency: "USD".to_owned(),
        stage: "lead".to_owned(),
        created_at: now(),
        updated_at: now(),
        ..Deal::default()
    }
}

async fn seed(app: &TestApp, count: usize) {
    let db = app.get::<Db>().await;
    let rows: Vec<Deal> = (0..count).map(deal_row).collect();
    for chunk in rows.chunks(200) {
        Deal::insert(&*db, chunk.to_vec()).await.unwrap();
    }
}

fn audited() -> navree_testing::TestAppBuilder {
    crate::servers::database(TestApp::builder())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("audit.routes", false)
        .module(navree_db::DatabaseModule)
        .module(navree_audit::AuditModule::from_config())
        .module(navree_access::AccessModule::for_root(options()))
        .module(CrmModule)
        .module(navree_helm::HelmModule::from_config())
}

async fn audit_rows(app: &TestApp, action: &str) -> Vec<Value> {
    let log = app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    navree_audit::AuditLog::list(
        &log,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data
    .into_iter()
    .filter(|row| row.action == action)
    .map(|row| serde_json::to_value(&row).unwrap())
    .collect()
}

const CSV: &str = "/admin/resources/deals/export.csv";

/// Reads the rest of a body: the bytes, and the error it ended with.
async fn rest<S, E>(mut body: S) -> (usize, Option<String>)
where
    S: futures::Stream<Item = Result<navree_http::body::Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let mut bytes = 0;
    while let Some(frame) = body.next().await {
        match frame {
            Ok(data) => bytes += data.len(),
            Err(error) => return (bytes, Some(error.to_string())),
        }
    }
    (bytes, None)
}

#[tokio::test]
async fn a_role_taken_away_ends_an_open_export() {
    let app = builder().build().await;
    seed(&app, 1600).await;
    let (id, manager) = user(&app, "manager").await;
    let response = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();

    let service = app.get::<navree_access::AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    service.remove_role(&uow, id, "manager").await.unwrap();
    uow.commit().await.unwrap();

    let (bytes, error) = rest(body).await;
    assert_eq!(bytes, 0, "no more rows");
    assert!(
        error.as_deref().is_some_and(|e| e.contains("stopped")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_user_who_loses_the_panel_loses_an_open_export() {
    let app = builder().build().await;
    seed(&app, 1600).await;
    let (id, manager) = user(&app, "manager").await;
    let response = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    let mut body = response.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    // The rules on deals stay: only the way into the panel goes.
    let service = app.get::<navree_access::AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    service.revoke(&uow, "manager", "helm.view").await.unwrap();
    uow.commit().await.unwrap();
    manager
        .get("/admin/api/resources/deals")
        .send()
        .await
        .assert_status(403);
    let _ = id;
    let (bytes, error) = rest(body).await;
    assert_eq!(bytes, 0);
    assert!(error.is_some());
}

#[tokio::test]
async fn an_export_ends_at_its_time_limit() {
    let app = builder().config("helm.export_max_secs", 1).build().await;
    seed(&app, 1600).await;
    let (_, manager) = user(&app, "manager").await;
    let response = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let (bytes, error) = rest(body).await;
    assert_eq!(bytes, 0, "no more rows after the limit");
    assert!(
        error.as_deref().is_some_and(|e| e.contains("time limit")),
        "{error:?}"
    );
    // A fast reader under the limit is not bothered: that is the default's job.
    let quick = builder().build().await;
    seed(&quick, 600).await;
    let (_, manager) = user(&quick, "manager").await;
    let response = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    let (bytes, error) = rest(response.into_body().into_data_stream()).await;
    assert!(bytes > 0);
    assert_eq!(error, None);
}

#[tokio::test]
async fn the_export_limit_must_make_sense() {
    let booted = builder()
        .config("helm.export_max_secs", 0)
        .try_build()
        .await;
    let Err(error) = booted else {
        panic!("0 would end every export at once");
    };
    let error = error.to_string();
    assert!(error.contains("helm.export_max_secs"), "{error}");
}

#[tokio::test]
async fn a_head_is_refused_and_writes_no_entry() {
    let app = audited().build().await;
    seed(&app, 3).await;
    let (_, manager) = user(&app, "manager").await;
    let head = manager
        .request(http::Method::HEAD, CSV)
        .header("accept", "text/html")
        .send()
        .await;
    assert_eq!(head.status(), 405);
    assert!(audit_rows(&app, "helm.export").await.is_empty());
    assert!(audit_rows(&app, "helm.export.finished").await.is_empty());
}

#[tokio::test]
async fn the_audit_log_says_how_many_rows_went_out_and_how_it_ended() {
    let app = audited().build().await;
    seed(&app, 1200).await;
    let (id, manager) = user(&app, "manager").await;
    let csv = manager.get(CSV).header("accept", "text/html").send().await;
    assert_eq!(csv.status(), 200);
    assert_eq!(csv.text().matches("\r\n").count(), 1201);
    let opened = audit_rows(&app, "helm.export").await;
    assert_eq!(opened.len(), 1, "{opened:?}");
    let finished = wait_for_entries(&app, 1).await;
    let metadata = &finished[0]["metadata"];
    assert_eq!(metadata["rows"], 1200, "{finished:?}");
    assert_eq!(metadata["outcome"], "finished", "{finished:?}");
    assert_eq!(metadata["resource"], "deals");
    assert_eq!(finished[0]["actor_id"], id.to_string(), "{finished:?}");

    // A client that takes the first chunk and goes.
    let gone = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    let mut body = gone.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    drop(body);
    let finished = wait_for_entries(&app, 2).await;
    let abandoned = finished
        .iter()
        .find(|row| row["metadata"]["outcome"] == "abandoned")
        .expect("the entry of the client that went");
    assert_eq!(abandoned["metadata"]["rows"], 500, "{abandoned}");

    // One whose rights go on the way.
    let open = manager
        .get(CSV)
        .header("accept", "text/html")
        .send_stream()
        .await;
    let mut body = open.into_body().into_data_stream();
    body.next().await.unwrap().unwrap();
    let service = app.get::<navree_access::AccessService>().await;
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    service.remove_role(&uow, id, "manager").await.unwrap();
    uow.commit().await.unwrap();
    let _ = rest(body).await;
    let finished = wait_for_entries(&app, 3).await;
    assert!(
        finished
            .iter()
            .any(|row| row["metadata"]["outcome"] == "stopped"),
        "{finished:?}"
    );
}

/// The closing entries, once there are `count`: an entry is written as the body
/// ends, which may be after the client has its last byte.
async fn wait_for_entries(app: &TestApp, count: usize) -> Vec<Value> {
    for _ in 0..100 {
        let rows = audit_rows(app, "helm.export.finished").await;
        if rows.len() >= count {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no {count} closing entries");
}

async fn fetched_from(client: &TestClient, path: &str, site: &str) -> u16 {
    client
        .get(path)
        .header("accept", "text/html")
        .header("sec-fetch-site", site)
        .send()
        .await
        .status()
        .as_u16()
}

#[tokio::test]
async fn another_site_of_the_domain_can_not_start_an_export_or_read_counts() {
    let app = audited().build().await;
    seed(&app, 2).await;
    let (_, manager) = user(&app, "manager").await;
    for site in ["same-site", "cross-site"] {
        assert_eq!(fetched_from(&manager, CSV, site).await, 403, "{site}");
        assert_eq!(
            fetched_from(&manager, "/admin/counts", site).await,
            403,
            "{site}"
        );
    }
    assert!(audit_rows(&app, "helm.export").await.is_empty());
    // The panel's own link, and a URL opened by hand.
    for site in ["same-origin", "none"] {
        assert_eq!(fetched_from(&manager, CSV, site).await, 200, "{site}");
        assert_eq!(
            fetched_from(&manager, "/admin/counts", site).await,
            200,
            "{site}"
        );
    }
}

#[tokio::test]
async fn a_spreadsheet_opens_a_file_as_what_it_says() {
    let app = app().await;
    let (_, manager) = user(&app, "manager").await;
    deal(
        &app,
        DealSeed {
            title: "Négative ＝ ünï",
            amount_cents: -123_460,
            ..DealSeed::default()
        },
    )
    .await;
    let body = manager
        .get(CSV)
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(
        body.starts_with('\u{feff}'),
        "a byte order mark, once, so Excel reads UTF-8: {body}"
    );
    assert_eq!(body.matches('\u{feff}').count(), 1);
    assert!(body.contains("Négative ＝ ünï"), "{body}");
    assert!(
        body.contains(",\"-$1,234.60\","),
        "an amount is a number, not guarded text: {body}"
    );
    assert!(!body.contains("'-$"), "{body}");
}

#[tokio::test]
async fn a_list_whose_query_orders_its_rows_can_be_exported() {
    let app = builder().module(RawOrderModule).build().await;
    seed(&app, 3).await;
    let (_, manager) = user(&app, "manager").await;
    let list = manager
        .get("/admin/api/resources/raw-order")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(ids(&list).len(), 3);
    let csv = manager
        .get("/admin/resources/raw-order/export.csv")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text();
    assert_eq!(csv.matches("\r\n").count(), 4, "{csv}");
}

/// A resource whose own query orders its rows by raw SQL.
struct RawOrder;

impl navree_kernel::Injectable for RawOrder {
    fn construct(
        _r: navree_kernel::Resolver,
    ) -> navree_kernel::BoxFuture<'static, Result<Self, navree_kernel::KernelError>> {
        Box::pin(async { Ok(RawOrder) })
    }
}

impl navree_helm::Resource for RawOrder {
    type Model = Deal;

    const LABEL: &'static str = "Raw";
    const SLUG: &'static str = "raw-order";
    const TITLE: &'static str = "title";
    const SIDEBAR_COUNT: bool = false;

    fn fields(&self) -> Vec<navree_helm::Field> {
        vec![navree_helm::Field::text("title")]
    }

    fn query(&self, query: navree_orm::Builder<Deal>) -> navree_orm::Builder<Deal> {
        query.order_by_raw("LOWER(title) DESC", Vec::<navree_orm::Value>::new())
    }
}

struct RawOrderModule;

impl navree_kernel::Module for RawOrderModule {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        navree_helm::ModuleBuilderHelmExt::resource::<RawOrder>(m);
    }
}
