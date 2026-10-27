//! Issue statuses: resolving, regressions, resolving in the next release
//! (two apps sharing a database with different release labels), ignoring
//! and reopening.

use navree_lighthouse::{IgnoreFor, IssueFilter, IssueStatus};
use navree_testing::{TestApp, temp_sqlite_url};

use crate::support::{builder, flushed};

async fn boom(app: &TestApp, n: u32) {
    app.client()
        .get(&format!("/api/demo/boom/{n}"))
        .send()
        .await
        .assert_status(500);
}

async fn only_issue(app: &TestApp) -> navree_lighthouse::Issue {
    let store = flushed(app).await;
    let page = store.issues(&IssueFilter::default()).await.unwrap();
    assert_eq!(page.data.len(), 1);
    page.data[0].issue.clone()
}

#[tokio::test]
async fn a_resolved_issue_that_happens_again_reopens_as_a_regression() {
    let app = builder().build().await;
    boom(&app, 1).await;
    let issue = only_issue(&app).await;
    let store = flushed(&app).await;
    let resolved = store
        .resolve(issue.id, false, Some("fixed in #42".into()))
        .await
        .unwrap();
    assert_eq!(resolved.status, IssueStatus::Resolved);
    assert_eq!(
        store
            .issue_detail(issue.id)
            .await
            .unwrap()
            .comment
            .as_deref(),
        Some("fixed in #42")
    );
    boom(&app, 2).await;
    let issue = only_issue(&app).await;
    assert_eq!(issue.status, IssueStatus::Open);
    assert_eq!(issue.count, 2);
    let detail = store.issue_detail(issue.id).await.unwrap();
    assert!(detail.regressed_at.is_some());
    assert!(detail.resolved_at.is_none());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn resolved_in_the_next_release_reopens_only_in_another_release() {
    let (url, _guard) = temp_sqlite_url();
    let in_release = |release: &str| {
        builder()
            .config("database.url", url.clone())
            .config("lighthouse.release", release)
    };
    let first = in_release("r1").build().await;
    boom(&first, 1).await;
    let issue = only_issue(&first).await;
    let store = flushed(&first).await;
    let resolved = store.resolve(issue.id, true, None).await.unwrap();
    assert_eq!(resolved.status, IssueStatus::ResolvedInNextRelease);
    assert_eq!(
        store
            .issue_detail(issue.id)
            .await
            .unwrap()
            .resolved_in_release
            .as_deref(),
        Some("r1")
    );
    // A straggler from the old release: counted, still resolved.
    boom(&first, 2).await;
    let issue = only_issue(&first).await;
    assert_eq!(issue.status, IssueStatus::ResolvedInNextRelease);
    assert_eq!(issue.count, 2);
    first.shutdown().await.unwrap();

    // The next release sees it again: a regression.
    let second = in_release("r2").build().await;
    boom(&second, 3).await;
    let issue = only_issue(&second).await;
    assert_eq!(issue.status, IssueStatus::Open);
    assert_eq!(issue.count, 3);
    let detail = flushed(&second).await.issue_detail(issue.id).await.unwrap();
    assert_eq!(detail.last_release.as_deref(), Some("r2"));
    assert_eq!(detail.first_release.as_deref(), Some("r1"));
    assert!(detail.regressed_at.is_some());
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn ignored_issues_count_and_reopen_after_their_occurrences() {
    let app = builder().build().await;
    boom(&app, 1).await;
    let issue = only_issue(&app).await;
    let store = flushed(&app).await;
    let ignored = store
        .ignore(issue.id, IgnoreFor::Occurrences(2), None)
        .await
        .unwrap();
    assert_eq!(ignored.status, IssueStatus::Ignored);
    boom(&app, 2).await;
    let issue = only_issue(&app).await;
    assert_eq!(issue.status, IssueStatus::Ignored);
    assert_eq!(issue.count, 2);
    assert_eq!(
        store.issue_detail(issue.id).await.unwrap().ignore_remaining,
        Some(1)
    );
    boom(&app, 3).await;
    let issue = only_issue(&app).await;
    assert_eq!(issue.status, IssueStatus::Open, "the second one reopens it");
    assert!(
        store
            .issue_detail(issue.id)
            .await
            .unwrap()
            .regressed_at
            .is_none(),
        "an ignored issue coming back is no regression"
    );

    store
        .ignore(issue.id, IgnoreFor::Forever, None)
        .await
        .unwrap();
    boom(&app, 4).await;
    assert_eq!(only_issue(&app).await.status, IssueStatus::Ignored);
    let reopened = store.reopen(issue.id).await.unwrap();
    assert_eq!(reopened.status, IssueStatus::Open);
    assert!(
        store
            .ignore(issue.id, IgnoreFor::Occurrences(0), None)
            .await
            .is_err()
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_issue_ignored_until_a_time_reopens_after_it() {
    let app = builder().build().await;
    boom(&app, 1).await;
    let issue = only_issue(&app).await;
    let store = flushed(&app).await;
    store
        .ignore(
            issue.id,
            IgnoreFor::Until(chrono::Utc::now() - chrono::TimeDelta::seconds(1)),
            None,
        )
        .await
        .unwrap();
    boom(&app, 2).await;
    assert_eq!(only_issue(&app).await.status, IssueStatus::Open);
    app.shutdown().await.unwrap();
}
