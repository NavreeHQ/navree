//! [`FieldCheck`]: the route authorizer checking something that is not a
//! route (a GraphQL field), with the record id taken from its arguments.

use std::collections::HashMap;
use std::sync::Arc;

use http::request::Parts;
use navree_access::{Authorized, FieldCheck, Read};
use navree_contracts::{AuditKind, AuthState};
use navree_db::{Db, Uow};
use navree_http::{AccessSpec, CanSpec, RouteAuthorizer, RouteMeta};
use navree_testing::{TestApp, principal};
use uuid::Uuid;

use crate::support::{Post, app, insert_post};

/// A field check of `read Post from id` by `user` (a guest when `None`).
async fn check(
    app: &TestApp,
    user: Option<Uuid>,
    id: &str,
    uow: &Uow,
) -> (Result<(), navree_http::AppError>, Parts) {
    let (mut parts, ()) = http::Request::new(()).into_parts();
    parts.extensions.insert(app.ctx().clone());
    parts.extensions.insert(AuthState(user.map(principal)));
    parts.extensions.insert(uow.clone());
    parts.extensions.insert(FieldCheck {
        path: "post".to_owned(),
        args: HashMap::from([("id".to_owned(), id.to_owned())]),
    });
    let meta = RouteMeta {
        access: AccessSpec {
            authenticated: true,
            can: Some(CanSpec {
                action: "read".to_owned(),
                subject: "Post".to_owned(),
                from: Some("id".to_owned()),
            }),
            ..AccessSpec::default()
        },
        method: "POST".to_owned(),
        full_path: "/graphql".to_owned(),
        ..RouteMeta::default()
    };
    let authorizer: Arc<dyn RouteAuthorizer> = app.get::<dyn RouteAuthorizer>().await;
    let result = authorizer.authorize(&mut parts, &meta).await;
    (result, parts)
}

#[tokio::test]
async fn a_field_check_reads_the_record_id_from_its_arguments() {
    let (app, _audit) = app().await;
    let author = Uuid::now_v7();
    let draft = insert_post(&app, "Draft", Some(author), "draft", false).await;
    let uow = app.get::<Db>().await.uow(None, None).await;

    let (result, mut parts) = check(&app, Some(author), &draft.id.to_string(), &uow).await;
    result.expect("the author may read the draft");
    let authorized =
        <Authorized<Post, Read> as axum::extract::FromRequestParts<()>>::from_request_parts(
            &mut parts,
            &(),
        )
        .await
        .expect("the record is handed over");
    assert_eq!(authorized.title, "Draft");

    let (missing, _) = check(&app, Some(author), &Uuid::now_v7().to_string(), &uow).await;
    assert_eq!(missing.unwrap_err().status.as_u16(), 404);
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn a_denied_field_check_names_the_field_and_leaves_the_unit_of_work_open() {
    let (app, audit) = app().await;
    let draft = insert_post(&app, "Draft", Some(Uuid::now_v7()), "draft", false).await;
    let stranger = Uuid::now_v7();
    let uow = app.get::<Db>().await.uow(None, None).await;

    let (result, _) = check(&app, Some(stranger), &draft.id.to_string(), &uow).await;
    assert_eq!(result.unwrap_err().status.as_u16(), 403);
    // The caller owns the unit of work (other fields may share it).
    assert!(!uow.is_finished().await);
    uow.rollback().await.unwrap();

    let entry = audit
        .entries()
        .into_iter()
        .find(|e| e.action == "access.denied")
        .expect("an access.denied entry");
    assert_eq!(entry.kind, AuditKind::Security);
    assert_eq!(entry.metadata["field"], "post");
    assert_eq!(entry.metadata["route"], "/graphql");
}

#[tokio::test]
async fn a_field_check_without_the_argument_is_a_server_error() {
    let (app, _audit) = app().await;
    let uow = app.get::<Db>().await.uow(None, None).await;
    let (mut parts, ()) = http::Request::new(()).into_parts();
    parts.extensions.insert(app.ctx().clone());
    parts
        .extensions
        .insert(AuthState(Some(principal(Uuid::now_v7()))));
    parts.extensions.insert(uow.clone());
    parts.extensions.insert(FieldCheck {
        path: "post".to_owned(),
        args: HashMap::new(),
    });
    let meta = RouteMeta {
        access: AccessSpec {
            can: Some(CanSpec {
                action: "read".to_owned(),
                subject: "Post".to_owned(),
                from: Some("id".to_owned()),
            }),
            ..AccessSpec::default()
        },
        ..RouteMeta::default()
    };
    let authorizer = app.get::<dyn RouteAuthorizer>().await;
    let error = authorizer.authorize(&mut parts, &meta).await.unwrap_err();
    assert_eq!(error.status.as_u16(), 500);
    uow.rollback().await.unwrap();
}
