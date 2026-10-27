//! A test app with the delivery pipeline: a temporary database, an app key,
//! events, the mail fake, and a permission-granting route authorizer.

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use http::request::Parts;
use navree_contracts::{AuthState, AuthVia, Principal};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db, Uow};
use navree_events::{EventBus, EventsModule};
use navree_http::{AppError, RouteAuthorizer, RouteMeta};
use navree_kernel::BoxFuture;
use navree_mail::{MailFake, MailModule, MailTransport, Mailer};
use navree_testing::{TestApp, TestAppBuilder};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::support::{FROM, MailTemplates, view_module};

/// The app and its fakes.
pub struct MailApp {
    pub app: TestApp,
    pub fake: MailFake,
    pub authorizer: Arc<GrantAuthorizer>,
}

impl MailApp {
    pub async fn mailer(&self) -> Arc<Mailer> {
        self.app.get::<Mailer>().await
    }

    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.app.get::<T>().await
    }

    /// A unit of work to pass to service writes; commit it after.
    pub async fn uow(&self) -> Uow {
        self.app.get::<Db>().await.uow(None, None).await
    }

    /// The JSON events named `name` emitted from now on.
    pub async fn events(&self, name: &str) -> broadcast::Receiver<serde_json::Value> {
        self.app.get::<EventBus>().await.subscribe_json(name)
    }
}

/// The base builder: database, app key, events, views, the mail module
/// from config with `mail.delivery.enabled`, and the fake transport.
pub fn builder(fake: &MailFake, authorizer: &Arc<GrantAuthorizer>) -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", navree_crypto::generate_key())
        .config("mail.from", FROM)
        .config("mail.delivery.enabled", true)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(view_module())
        .module(MailTemplates)
        .module(MailModule::from_config())
        .override_value::<dyn MailTransport>(fake.transport())
        .override_value::<dyn RouteAuthorizer>(authorizer.clone())
        .fake_auth()
}

/// Boots the delivery app, with `configure` applied to the builder.
pub async fn mail_app(configure: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> MailApp {
    let fake = MailFake::new();
    let authorizer = Arc::new(GrantAuthorizer::default());
    let app = configure(builder(&fake, &authorizer)).build().await;
    MailApp {
        app,
        fake,
        authorizer,
    }
}

pub fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

/// Lets a request through when its principal was granted every permission
/// the route requires; 403 otherwise.
#[derive(Default)]
pub struct GrantAuthorizer {
    granted: Mutex<HashSet<(Uuid, String)>>,
}

impl GrantAuthorizer {
    pub fn grant(&self, user: Uuid, permission: &str) {
        self.granted
            .lock()
            .unwrap()
            .insert((user, permission.to_owned()));
    }
}

impl RouteAuthorizer for GrantAuthorizer {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        let allowed = user.is_some_and(|user| {
            let granted = self.granted.lock().unwrap();
            meta.access
                .permissions
                .iter()
                .all(|p| granted.contains(&(user, p.clone())))
        });
        Box::pin(async move {
            if allowed {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}
