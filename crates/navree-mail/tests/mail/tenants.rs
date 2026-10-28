//! Tenant transports: domain, workspace, organization and platform tiers.

use std::any::Any;
use std::sync::Arc;

use navree_mail::entity::TransportRow;
use navree_mail::{
    ArrayTransport, DeliveryStatus, MailError, MailScope, MailTransport, Message, TransportConfig,
    TransportOwner, TransportResolver, TransportTier,
};
use serde_json::json;
use uuid::Uuid;

use crate::harness::{MailApp, mail_app};

fn hello(from: &str) -> Message {
    Message::new()
        .from(from)
        .to("ada@example.com")
        .subject("Hi")
        .text("Hi")
}

fn array_of(transport: &Arc<dyn MailTransport>) -> ArrayTransport {
    (transport.as_ref() as &dyn Any)
        .downcast_ref::<ArrayTransport>()
        .expect("an array transport")
        .clone()
}

async fn set(mail: &MailApp, owner: TransportOwner, config: &TransportConfig) {
    let uow = mail.uow().await;
    mail.get::<TransportResolver>()
        .await
        .set(&uow, owner, config)
        .await
        .expect("set");
    uow.commit().await.expect("commit");
}

#[tokio::test]
async fn the_most_specific_tier_wins() {
    let mail = mail_app(|b| b).await;
    let (workspace, organization) = (Uuid::now_v7(), Uuid::now_v7());
    let scope = MailScope::workspace(workspace).with_organization(organization);
    let mailer = mail.mailer().await;
    let tier = |from: &'static str| {
        let mailer = mailer.clone();
        let scope = scope.clone();
        async move {
            mailer
                .for_scope(&scope)
                .deliver(&hello(from))
                .await
                .expect("deliver")
                .tier
                .expect("a tier")
        }
    };

    assert_eq!(tier("hello@acme.test").await, TransportTier::Config);
    set(&mail, TransportOwner::Platform, &TransportConfig::Array).await;
    assert_eq!(tier("hello@acme.test").await, TransportTier::Platform);
    set(
        &mail,
        TransportOwner::organization(organization),
        &TransportConfig::Array,
    )
    .await;
    assert_eq!(tier("hello@acme.test").await, TransportTier::Organization);
    set(
        &mail,
        TransportOwner::workspace(workspace),
        &TransportConfig::Array,
    )
    .await;
    assert_eq!(tier("hello@acme.test").await, TransportTier::Workspace);
    set(
        &mail,
        TransportOwner::domain("ACME.test"),
        &TransportConfig::Array,
    )
    .await;
    assert_eq!(tier("Acme <hello@acme.test>").await, TransportTier::Domain);
    // Another sender domain skips the domain tier.
    assert_eq!(tier("hello@other.test").await, TransportTier::Workspace);
    // The fake (the configured transport) sent only the first message.
    assert_eq!(mail.fake.sent().len(), 1);
}

#[tokio::test]
async fn a_workspaces_domain_is_not_used_for_another_workspace() {
    let mail = mail_app(|b| b).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    set(
        &mail,
        TransportOwner::workspace_domain("acme.test", a),
        &TransportConfig::Array,
    )
    .await;
    let mailer = mail.mailer().await;

    let for_a = mailer
        .for_scope(&MailScope::workspace(a))
        .deliver(&hello("hello@acme.test"))
        .await
        .unwrap();
    let for_b = mailer
        .for_scope(&MailScope::workspace(b))
        .deliver(&hello("hello@acme.test"))
        .await
        .unwrap();

    assert_eq!(for_a.tier, Some(TransportTier::Domain));
    assert_eq!(for_b.tier, Some(TransportTier::Config));
}

#[tokio::test]
async fn the_resolved_transport_delivers_and_is_reused() {
    let mail = mail_app(|b| b).await;
    let workspace = Uuid::now_v7();
    set(
        &mail,
        TransportOwner::workspace(workspace),
        &TransportConfig::Array,
    )
    .await;
    let resolver = mail.get::<TransportResolver>().await;
    let scope = MailScope::workspace(workspace);

    let first = resolver
        .resolve(&scope, None)
        .await
        .unwrap()
        .expect("resolved");
    let second = resolver
        .resolve(&scope, None)
        .await
        .unwrap()
        .expect("resolved");
    assert!(Arc::ptr_eq(&first.transport, &second.transport), "cached");

    let report = mail
        .mailer()
        .await
        .for_scope(&scope)
        .deliver(&hello("hello@acme.test"))
        .await
        .unwrap();
    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("array"));
    array_of(&first.transport).assert_sent(|m| m.subject == "Hi");
    mail.fake.assert_nothing_sent();
}

#[tokio::test]
async fn disabled_and_removed_transports_are_skipped() {
    let mail = mail_app(|b| b).await;
    let workspace = Uuid::now_v7();
    let owner = TransportOwner::workspace(workspace);
    set(&mail, owner.clone(), &TransportConfig::Array).await;
    let resolver = mail.get::<TransportResolver>().await;
    let stored = resolver.get(&owner).await.unwrap().expect("stored");
    assert_eq!(stored.kind, "array");
    assert_eq!(stored.tier, TransportTier::Workspace);
    assert!(stored.enabled);
    let scope = MailScope::workspace(workspace);

    let uow = mail.uow().await;
    resolver.set_enabled(&uow, stored.id, false).await.unwrap();
    uow.commit().await.unwrap();
    assert!(resolver.resolve(&scope, None).await.unwrap().is_none());

    let uow = mail.uow().await;
    resolver.set_enabled(&uow, stored.id, true).await.unwrap();
    assert!(resolver.remove(&uow, &owner).await.unwrap());
    assert!(!resolver.remove(&uow, &owner).await.unwrap());
    uow.commit().await.unwrap();
    assert!(resolver.resolve(&scope, None).await.unwrap().is_none());
}

#[tokio::test]
async fn settings_are_stored_encrypted_and_replaced_in_place() {
    let mail = mail_app(|b| b).await;
    let owner = TransportOwner::Platform;
    let ses: TransportConfig =
        serde_json::from_value(json!("ses://AKID:topsecret@eu-west-1")).unwrap();
    set(&mail, owner.clone(), &ses).await;
    let resolver = mail.get::<TransportResolver>().await;
    let stored = resolver.get(&owner).await.unwrap().unwrap();

    let db = mail.get::<navree_db::Db>().await;
    let model = navree_db::record::find::<TransportRow, _>(&*db, stored.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !model.config.contains("topsecret"),
        "the settings are encrypted"
    );
    assert_eq!(resolver.config(stored.id).await.unwrap(), ses);

    set(&mail, owner.clone(), &TransportConfig::Log).await;
    let replaced = resolver.get(&owner).await.unwrap().unwrap();
    assert_eq!(replaced.id, stored.id);
    assert_eq!(replaced.kind, "log");
    assert_eq!(
        resolver
            .list(Some(TransportTier::Platform))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn tenants_cannot_point_transports_at_private_networks_or_other_endpoints() {
    let mail = mail_app(|b| b).await;
    let resolver = mail.get::<TransportResolver>().await;
    let uow = mail.uow().await;
    let owner = TransportOwner::workspace(Uuid::now_v7());

    let local = TransportConfig::smtp("smtp://u:p@127.0.0.1:2525");
    let error = resolver.set(&uow, owner.clone(), &local).await.unwrap_err();
    assert!(
        matches!(&error, MailError::Unusable(reason) if reason.contains("private or reserved")),
        "{error}"
    );
    let error = resolver.usable(&local).await.unwrap_err();
    assert!(matches!(error, MailError::Unusable(_)), "{error}");

    let endpoint: TransportConfig = serde_json::from_value(json!(
        "resend://re_1@api.resend.com?endpoint=https://evil.test"
    ))
    .unwrap();
    let error = resolver.set(&uow, owner, &endpoint).await.unwrap_err();
    assert!(error.to_string().contains("endpoint"), "{error}");

    // The platform may use an internal relay.
    resolver
        .set(&uow, TransportOwner::Platform, &local)
        .await
        .expect("the platform tier is trusted");
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn usable_runs_the_transports_check() {
    let mail = mail_app(|b| b).await;
    let resolver = mail.get::<TransportResolver>().await;

    resolver
        .usable(&TransportConfig::Array)
        .await
        .expect("usable");

    let unreachable = TransportConfig::smtp("smtp://u:p@smtp.invalid:2525");
    let error = resolver.usable(&unreachable).await.unwrap_err();
    assert!(matches!(error, MailError::Unusable(_)), "{error}");

    let invalid = TransportConfig::Failover(Vec::new());
    assert!(matches!(
        resolver.usable(&invalid).await,
        Err(MailError::Config(_))
    ));
}

#[tokio::test]
async fn a_domain_owner_needs_a_domain_name() {
    let mail = mail_app(|b| b).await;
    let uow = mail.uow().await;

    let error = mail
        .get::<TransportResolver>()
        .await
        .set(
            &uow,
            TransportOwner::domain("localhost"),
            &TransportConfig::Array,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("not a domain name"), "{error}");
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn via_bypasses_tenant_transports() {
    let mail = mail_app(|b| b.config("mail.transports.simulation", "array")).await;
    let workspace = Uuid::now_v7();
    set(
        &mail,
        TransportOwner::workspace(workspace),
        &TransportConfig::Log,
    )
    .await;

    let report = mail
        .mailer()
        .await
        .for_scope(&MailScope::workspace(workspace))
        .via("simulation")
        .deliver(&hello("hello@acme.test"))
        .await
        .unwrap();

    assert_eq!(report.transport, Some("array"));
    assert_eq!(report.tier, Some(TransportTier::Config));
}

#[tokio::test]
async fn tenant_resolution_can_be_turned_off() {
    let mail = mail_app(|b| b.config("mail.delivery.tenants", false)).await;
    set(&mail, TransportOwner::Platform, &TransportConfig::Array).await;

    let report = mail
        .mailer()
        .await
        .deliver(&hello("hello@acme.test"))
        .await
        .unwrap();

    assert_eq!(report.tier, Some(TransportTier::Config));
    assert_eq!(report.transport, Some("fake"));
}
