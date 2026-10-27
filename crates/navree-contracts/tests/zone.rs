//! The zone-source contract: levels, the request view and the multi-binding.

use std::sync::Arc;

use navree_contracts::{
    AuthVia, BoxError, BoxFuture, Principal, ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource,
};
use navree_kernel::{Factory, Module, ModuleBuilder, Navree};
use uuid::Uuid;

#[test]
fn levels_resolve_from_the_user_up() {
    let mut levels = vec![
        ZoneLevel::Organization,
        ZoneLevel::User,
        ZoneLevel::Workspace,
    ];
    levels.sort();
    assert_eq!(
        levels,
        [ZoneLevel::User, ZoneLevel::Workspace, ZoneLevel::Organization]
    );
    assert_eq!(ZoneLevel::Workspace.as_str(), "workspace");
    assert_eq!(ZoneLevel::ALL.len(), 3);
    assert_eq!(
        serde_json::to_string(&ZoneLevel::Organization).unwrap(),
        r#""organization""#
    );
    assert_eq!(
        serde_json::from_str::<ZoneLevel>(r#""user""#).unwrap(),
        ZoneLevel::User
    );
}

#[test]
fn owners_carry_an_optional_zone_and_country() {
    let id = Uuid::now_v7();
    let owner = ZoneOwner::new(id).zone("Asia/Kolkata").country("IN");
    assert_eq!(owner.id, id);
    assert_eq!(owner.zone.as_deref(), Some("Asia/Kolkata"));
    assert_eq!(owner.country.as_deref(), Some("IN"));
    let bare = ZoneOwner::new(id);
    assert!(bare.zone.is_none() && bare.country.is_none());
}

#[derive(Debug, PartialEq)]
struct Head {
    workspace: &'static str,
}

#[test]
fn a_request_exposes_its_principal_and_its_head() {
    let user = Uuid::now_v7();
    let principal = Principal {
        user_id: user,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    };
    let mut head = Head { workspace: "acme" };
    let mut request = ZoneRequest::new(Some(&principal), &mut head);
    assert_eq!(request.principal().map(|p| p.user_id), Some(user));
    assert_eq!(request.user_id(), Some(user));
    assert_eq!(
        request.head::<Head>().map(|h| h.workspace),
        Some("acme"),
        "the head comes back as its own type"
    );
    assert!(request.head::<String>().is_none(), "other types do not");
    request.head::<Head>().unwrap().workspace = "globex";
    assert_eq!(head.workspace, "globex");

    let mut detached = ZoneRequest::detached(None);
    assert!(detached.principal().is_none());
    assert!(detached.head::<Head>().is_none());
}

/// A source that knows one user's zone.
struct OneUser(Uuid);

impl ZoneSource for OneUser {
    fn level(&self) -> ZoneLevel {
        ZoneLevel::User
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            Ok(request
                .user_id()
                .filter(|id| *id == self.0)
                .map(|id| ZoneOwner::new(id).zone("Europe/Paris")))
        })
    }

    fn zone_of(&self, owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            Ok((owner == self.0).then(|| ZoneOwner::new(owner).zone("Europe/Paris")))
        })
    }
}

struct ZonesModule(Uuid);

impl Module for ZonesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let user = self.0;
        m.multi(Factory::<dyn ZoneSource>::new(move |_| async move {
            Ok(Arc::new(OneUser(user)) as Arc<dyn ZoneSource>)
        }));
    }
}

#[tokio::test]
async fn sources_are_a_multi_binding_and_list_no_owners_by_default() {
    let user = Uuid::now_v7();
    let app = Navree::app()
        .module(ZonesModule(user))
        .boot()
        .await
        .expect("boot");
    let sources = app.ctx().get_all::<dyn ZoneSource>().await.unwrap();
    assert_eq!(sources.len(), 1);
    let source = &sources[0];
    assert_eq!(source.level(), ZoneLevel::User);
    assert!(source.owners().await.unwrap().is_empty());
    assert_eq!(
        source.zone_of(user).await.unwrap().and_then(|o| o.zone),
        Some("Europe/Paris".to_owned())
    );
    assert!(source.zone_of(Uuid::now_v7()).await.unwrap().is_none());

    let principal = Principal {
        user_id: user,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    };
    let mut request = ZoneRequest::detached(Some(&principal));
    let owner = source.zone_of_request(&mut request).await.unwrap().unwrap();
    assert_eq!(owner.id, user);
}
