//! [`Lazy`]: a provider handle resolved on first use.

use std::fmt;
use std::sync::{Arc, Weak};

use tokio::sync::OnceCell;

use crate::container::{AppInner, RequestScope};
use crate::{KernelError, ModuleId, ProviderKey, Resolver};

/// A handle to the unnamed provider of `T`, resolved (with the visibility of
/// the module that created it) on the first [`get`](Lazy::get) and then cached.
///
/// Created by [`Resolver::lazy`]. It is not a dependency edge, so it is the
/// escape hatch for provider cycles. It holds the app weakly, so it does not
/// keep a dropped app alive.
pub struct Lazy<T: ?Sized> {
    app: Weak<AppInner>,
    module: ModuleId,
    scope: Option<RequestScope>,
    cell: OnceCell<Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> Lazy<T> {
    pub(crate) fn new(app: &Arc<AppInner>, module: ModuleId, scope: Option<RequestScope>) -> Self {
        Self {
            app: Arc::downgrade(app),
            module,
            scope,
            cell: OnceCell::new(),
        }
    }

    /// Resolves the provider on first call; later calls return the same `Arc`.
    pub async fn get(&self) -> Result<Arc<T>, KernelError> {
        self.cell
            .get_or_try_init(|| async {
                let app = self
                    .app
                    .upgrade()
                    .ok_or_else(|| KernelError::Construction {
                        provider: ProviderKey::of::<T>().to_string(),
                        source: "the app that created this lazy handle has been dropped".into(),
                    })?;
                Resolver::new(app, self.module, self.scope.clone())
                    .get::<T>()
                    .await
            })
            .await
            .cloned()
    }
}

impl<T: ?Sized> fmt::Debug for Lazy<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lazy")
            .field("type", &std::any::type_name::<T>())
            .field("module", &self.module.name())
            .field("resolved", &self.cell.initialized())
            .finish()
    }
}
