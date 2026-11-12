//! Sealed one-time tickets (`dyn CallTickets`, navree-auth's
//! `SealedTickets`): issued while a page is built for the browser's
//! session, redeemed once by the route the result is posted to.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_auth::AuthError;
use navree_contracts::{
    AuthState, CallTickets, Clock as _, FixedClock, TicketCheck, TicketHolder, TicketRequest,
};
use navree_http::{AppError, Controller, Extension, Json, ModuleBuilderHttpExt, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_session::{SessionModule, SessionOptions};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde::Deserialize;
use serde_json::json;

use crate::support::{self, PASSWORD, create_user};

/// A page that hands out tickets and the routes results are posted to.
struct OrdersController {
    tickets: Arc<dyn CallTickets>,
}

impl Injectable for OrdersController {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(OrdersController {
                tickets: r.get::<dyn CallTickets>().await?,
            })
        })
    }
}

#[derive(Deserialize)]
struct Issue {
    feature: String,
    path: String,
}

#[derive(Deserialize)]
struct Posted {
    ticket: String,
}

impl Controller for OrdersController {
    fn prefix(&self) -> &'static str {
        "/orders"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        // What a page does while it is built: a ticket for the call it
        // carries, for the request's own session.
        let tickets = self.tickets.clone();
        let issue = move |state: Extension<AuthState>, Json(input): Json<Issue>| {
            let tickets = tickets.clone();
            async move {
                let holder = TicketHolder::of(state.0.0.as_ref());
                let request = TicketRequest::new(holder, input.feature, "POST", &input.path)
                    .binding("sku=PB-1042");
                let issued = tickets
                    .issue(&request)
                    .map_err(|error| AuthError::from(error).into_app_error())?;
                Ok::<_, AppError>(Json(json!({
                    "ticket": issued.ticket,
                    "expires_at": issued.expires_at,
                })))
            }
        };
        // What the continuation does: redeem the ticket at its own route.
        let tickets = self.tickets.clone();
        let scanned = move |state: Extension<AuthState>,
                            Path(id): Path<String>,
                            Json(input): Json<Posted>| {
            let tickets = tickets.clone();
            async move {
                let check = TicketCheck::new(TicketHolder::of(state.0.0.as_ref()), "scan_code")
                    .at("POST", &format!("/api/orders/{id}/scanned"));
                let redeemed = tickets
                    .redeem(&input.ticket, &check)
                    .await
                    .map_err(|error| AuthError::from(error).into_app_error())?;
                Ok::<_, AppError>(Json(json!({ "binding": redeemed.binding })))
            }
        };
        vec![
            RouteDef::on(Method::POST, "/tickets", issue).public(),
            RouteDef::on(Method::POST, "/{id}/scanned", scanned).public(),
        ]
    }
}

struct OrdersModule;

impl Module for OrdersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<OrdersController>();
    }
}

async fn app(clock: &Arc<FixedClock>) -> TestApp {
    support::clocked(clock)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(OrdersModule)
        .build()
        .await
}

/// A browser signed in as `email` on its session cookie.
async fn browser(app: &TestApp, email: &str) -> TestClient {
    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/api/auth/session/login")
        .xsrf()
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    client
}

/// A ticket for the scanner on order `order`, posted to `path`.
async fn ticket(client: &TestClient, feature: &str, path: &str) -> String {
    client
        .post("/api/orders/tickets")
        .xsrf()
        .json(&json!({ "feature": feature, "path": path }))
        .send()
        .await
        .assert_status(200)
        .json_value()["ticket"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn post_result(client: &TestClient, order: &str, ticket: &str) -> TestResponse {
    client
        .post(&format!("/api/orders/{order}/scanned"))
        .xsrf()
        .json(&json!({ "ticket": ticket }))
        .send()
        .await
}

fn refused(response: TestResponse) {
    response.assert_error_code(422, "invalid_ticket");
}

#[tokio::test]
async fn a_ticket_is_redeemed_once_at_its_route() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let ticket = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned?from=list").await;
    post_result(&phone, "A-1042", &ticket)
        .await
        .assert_status(200)
        .assert_json_path("binding", json!("sku=PB-1042"));
    refused(post_result(&phone, "A-1042", &ticket).await);
}

#[tokio::test]
async fn a_forged_ticket_is_refused() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let real = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned").await;
    let mut changed = real.clone().into_bytes();
    let middle = changed.len() / 2;
    changed[middle] = if changed[middle] == b'A' { b'B' } else { b'A' };
    for forged in [
        String::new(),
        "v1.key.nonce.ciphertext".to_owned(),
        String::from_utf8(changed).unwrap(),
        format!("{real}x"),
        real[..real.len() - 4].to_owned(),
    ] {
        refused(post_result(&phone, "A-1042", &forged).await);
    }
    // The real one still works: a refused forgery burns nothing.
    post_result(&phone, "A-1042", &real)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn a_ticket_used_twice_is_refused_even_racing() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    for _ in 0..5 {
        let ticket = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned").await;
        let (first, second) = tokio::join!(
            post_result(&phone, "A-1042", &ticket),
            post_result(&phone, "A-1042", &ticket)
        );
        let statuses = [first.status().as_u16(), second.status().as_u16()];
        assert!(
            statuses == [200, 422] || statuses == [422, 200],
            "{statuses:?}"
        );
    }
}

#[tokio::test]
async fn a_ticket_from_another_session_is_refused() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    create_user(&app, "harpreet@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let laptop = browser(&app, "simran@example.com").await;
    let other_user = browser(&app, "harpreet@example.com").await;
    let guest = app.client();
    guest.csrf_cookie().await;
    let ticket = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned").await;
    for client in [&laptop, &other_user, &guest] {
        refused(post_result(client, "A-1042", &ticket).await);
    }
    // After signing out, the session's tickets are no one's.
    phone
        .post("/api/auth/logout")
        .xsrf()
        .send()
        .await
        .assert_status(204);
    phone.csrf_cookie().await;
    refused(post_result(&phone, "A-1042", &ticket).await);
}

#[tokio::test]
async fn a_ticket_sent_to_another_route_is_refused() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let ticket = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned").await;
    refused(post_result(&phone, "A-9999", &ticket).await);
    post_result(&phone, "A-1042", &ticket)
        .await
        .assert_status(200);
}

#[tokio::test]
async fn an_expired_ticket_is_refused() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let ticket = ticket(&phone, "scan_code", "/api/orders/A-1042/scanned").await;
    clock.advance(TicketRequest::DEFAULT_TTL + Duration::from_secs(1));
    refused(post_result(&phone, "A-1042", &ticket).await);
}

#[tokio::test]
async fn a_ticket_for_another_feature_is_refused() {
    let clock = support::clock();
    let app = app(&clock).await;
    create_user(&app, "simran@example.com").await;
    let phone = browser(&app, "simran@example.com").await;
    let ticket = ticket(&phone, "locate_once", "/api/orders/A-1042/scanned").await;
    refused(post_result(&phone, "A-1042", &ticket).await);
}

#[tokio::test]
async fn the_app_reaches_tickets_as_a_contract() {
    let clock = support::clock();
    let app = app(&clock).await;
    let tickets = app.get::<dyn CallTickets>().await;
    let request = TicketRequest::new(TicketHolder::guest(), "share", "POST", "/share/done");
    let issued = tickets.issue(&request).unwrap();
    assert!(issued.expires_at > clock.now());
    let check = TicketCheck::new(TicketHolder::guest(), "share").at("POST", "/share/done/");
    let redeemed = tickets.redeem(&issued.ticket, &check).await.unwrap();
    assert_eq!(redeemed.path, "/share/done");
    assert_eq!(redeemed.feature, "share");
    assert_eq!(redeemed.binding, None);
}
