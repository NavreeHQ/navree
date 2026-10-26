//! What resource code gets from the panel: [`ActionContext`],
//! [`SaveContext`] and [`PageContext`].

use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::{AppContext, ModuleId};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use uuid::Uuid;

/// What the three contexts share.
pub(crate) struct ContextInner {
    pub(crate) app: AppContext,
    /// The module that registered the resource or page: services resolve
    /// with its visibility.
    pub(crate) module: ModuleId,
    pub(crate) uow: Uow,
    pub(crate) user_id: Uuid,
    pub(crate) input: Map<String, Value>,
}

impl ContextInner {
    async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, AppError> {
        self.app
            .resolver_for(self.module)
            .get::<T>()
            .await
            .map_err(AppError::internal)
    }
}

macro_rules! shared_context {
    ($name:ident) => {
        impl $name {
            pub(crate) fn new(inner: ContextInner) -> Self {
                Self(Arc::new(inner))
            }

            /// The unit of work of the request (or of the queued job): every
            /// write through it commits or rolls back together.
            pub fn uow(&self) -> &Uow {
                &self.0.uow
            }

            /// The signed-in user.
            pub fn user_id(&self) -> Uuid {
                self.0.user_id
            }

            /// The app.
            pub fn app(&self) -> &AppContext {
                &self.0.app
            }

            /// Resolves a service, with the visibility of the module that
            /// registered the resource (its own providers and its imports).
            pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, AppError> {
                self.0.get::<T>().await
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("user_id", &self.0.user_id)
                    .finish_non_exhaustive()
            }
        }
    };
}

/// What an action's handler gets besides the records: the unit of work, the
/// user, services and the values of the action's form.
#[derive(Clone)]
pub struct ActionContext(Arc<ContextInner>);

shared_context!(ActionContext);

impl ActionContext {
    /// The value the user entered in the action's field `name`
    /// (`Action::fields`), deserialized; a 400 naming the field when it is
    /// missing or of another type.
    pub fn input<T: DeserializeOwned>(&self, name: &str) -> Result<T, AppError> {
        let value = self.0.input.get(name).cloned().unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(|_| {
            let mut errors = std::collections::BTreeMap::new();
            errors.insert(name.to_owned(), vec![format!("{name} is invalid")]);
            AppError::validation(errors)
        })
    }

    /// Every value of the action's form.
    pub fn inputs(&self) -> &Map<String, Value> {
        &self.0.input
    }
}

/// What `Resource::create`, `update` and `delete` get: the unit of work,
/// the user and services.
#[derive(Clone)]
pub struct SaveContext(Arc<ContextInner>);

shared_context!(SaveContext);

/// What a custom page's data function gets: the unit of work, the user and
/// services.
#[derive(Clone)]
pub struct PageContext(Arc<ContextInner>);

shared_context!(PageContext);
