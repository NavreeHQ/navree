//! The migration that lets several workspaces claim one host: domains that
//! were verified keep their hosts, the schema allows claims, and rolling it
//! back keeps the verified domains and drops the claims the old schema had
//! no room for.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Migrator, RollbackOptions};
use navree_domains::{DomainStatus, NewDomain, models};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::support::World;

const CLAIMS: &str = "m20261028_100005_claim_domains_by_proof";

/// Rolls back to the schema before the claims migration, with a verified
/// domain and two claims written before, then migrates up again and rolls
/// back once more. Runs on whatever database `world` uses.
pub async fn roundtrip(world: &World) {
    let ctx = world.app.ctx();
    let db = world.db().await;
    let domains = world.domains().await;
    let acme = world.workspace(Uuid::now_v7(), "Acme").await;
    let globex = world.workspace(Uuid::now_v7(), "Globex").await;
    let verified = world.verified(acme, "crm.acme-corp.test").await;
    let pending = world.add(acme, "go.acme-corp.test").await;
    let rival = domains
        .add(globex, NewDomain::new("go.acme-corp.test"))
        .await
        .unwrap();
    assert_ne!(pending.id, rival.id);

    // Before the claims migration: one row per host, unique.
    let status = Migrator::status(ctx).await.unwrap();
    let at = status
        .iter()
        .position(|entry| entry.module == "domains" && entry.name == CLAIMS)
        .expect("the claims migration is registered");
    let reverted = Migrator::rollback_with(
        ctx,
        &RollbackOptions {
            step: Some(u32::try_from(status.len() - at).unwrap()),
            ..RollbackOptions::default()
        },
    )
    .await
    .unwrap();
    assert!(
        reverted.labels().contains(&format!("domains/{CLAIMS}")),
        "{:?}",
        reverted.labels()
    );
    let schema = Schema::new(db.conn());
    assert!(
        !schema
            .has_column("custom_domains", "verified_host")
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_column("custom_domains", "transfer")
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("custom_domains", ["host"], Some(IndexKind::Unique))
            .await
            .unwrap(),
        "a host has one row again"
    );

    // Up again: the verified domain keeps its host, claims are possible.
    Migrator::up(ctx).await.unwrap();
    assert!(
        schema
            .has_column("custom_domains", "verified_host")
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("custom_domains", ["verified_host"], Some(IndexKind::Unique))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index(
                "custom_domains",
                ["workspace_id", "host"],
                Some(IndexKind::Unique)
            )
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_index("custom_domains", ["host"], Some(IndexKind::Unique))
            .await
            .unwrap()
    );
    let owner = models::Domain::query()
        .where_(models::Domain::VERIFIED_HOST.eq("crm.acme-corp.test"))
        .first(&*db)
        .await
        .unwrap()
        .expect("the verified domain owns its host after the migration");
    assert_eq!(owner.id, verified.id);
    assert_eq!(owner.status, DomainStatus::Verified.as_str());
    // The claims written before were dropped by the rollback (the old
    // schema had no room for two rows of one host).
    assert!(
        domains
            .claims("go.acme-corp.test")
            .await
            .unwrap()
            .is_empty()
    );
    domains
        .add(acme, NewDomain::new("go.acme-corp.test"))
        .await
        .unwrap();
    domains
        .add(globex, NewDomain::new("go.acme-corp.test"))
        .await
        .unwrap();
    assert_eq!(domains.claims("go.acme-corp.test").await.unwrap().len(), 2);
}

#[tokio::test]
async fn existing_domains_keep_their_hosts_and_the_migration_rolls_back() {
    let world = World::new().await;
    roundtrip(&world).await;
}
