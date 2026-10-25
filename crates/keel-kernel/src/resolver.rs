//! [`Resolver`]: resolves providers as seen from one module.

use std::fmt;
use std::sync::Arc;

use crate::builder::Instance;
use crate::container::{AppInner, RequestScope, app_identity};
use crate::graph::Visible;
use crate::{AppContext, KernelError, Lazy, ModuleId, ProviderKey, Scope};

/// Resolves providers with the visibility of one module: its own providers,
/// the exports of the modules it imports and the exports of global modules.
///
/// Every provider constructor receives a `Resolver` for the module that owns
/// the provider. It carries the optional [`RequestScope`] and the resolution
/// stack used to detect cycles and scope inversions at runtime.
#[derive(Clone)]
pub struct Resolver {
    pub(crate) inner: Arc<AppInner>,
    module: ModuleId,
    module_index: Option<usize>,
    scope: Option<RequestScope>,
    stack: Stack,
}

impl Resolver {
    pub(crate) fn new(inner: Arc<AppInner>, module: ModuleId, scope: Option<RequestScope>) -> Self {
        let module_index = inner.graph.module_of(module);
        Self {
            inner,
            module,
            module_index,
            scope,
            stack: Stack::default(),
        }
    }

    /// Resolves the unnamed provider of `T`.
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, KernelError> {
        self.get_key::<T>(&ProviderKey::of::<T>()).await
    }

    /// Resolves the provider of `T` registered under `name`.
    pub async fn get_named<T: ?Sized + Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Result<Arc<T>, KernelError> {
        self.get_key::<T>(&ProviderKey::named::<T>(name.to_owned()))
            .await
    }

    /// Resolves every multi-binding of `T` in the app (see
    /// [`ModuleBuilder::multi`](crate::ModuleBuilder::multi)), in module dependency order.
    pub async fn get_all<T: ?Sized + Send + Sync + 'static>(
        &self,
    ) -> Result<Vec<Arc<T>>, KernelError> {
        let key = ProviderKey::of::<T>();
        let mut all = Vec::new();
        for &index in self.inner.graph.multi_of(key.type_id()) {
            all.push(self.resolve_as::<T>(index, &key).await?);
        }
        Ok(all)
    }

    /// A handle that resolves the unnamed provider of `T` on its first
    /// [`get`](Lazy::get). It adds no edge to the dependency graph, so it
    /// breaks cycles; resolve it after boot, not inside a constructor.
    pub fn lazy<T: ?Sized + Send + Sync + 'static>(&self) -> Lazy<T> {
        Lazy::new(&self.inner, self.module, self.scope.clone())
    }

    /// A handle to the whole app.
    pub fn app(&self) -> AppContext {
        AppContext::from_inner(self.inner.clone())
    }

    /// The module whose visibility this resolver uses.
    pub fn module(&self) -> ModuleId {
        self.module
    }

    async fn get_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: &ProviderKey,
    ) -> Result<Arc<T>, KernelError> {
        let index = self.lookup(key)?;
        self.resolve_as::<T>(index, key).await
    }

    /// Resolves provider `index`, whose key is `key`, as an `Arc<T>`.
    pub(crate) async fn resolve_as<T: ?Sized + Send + Sync + 'static>(
        &self,
        index: usize,
        key: &ProviderKey,
    ) -> Result<Arc<T>, KernelError> {
        let instance = self.resolve_index(index).await?;
        downcast::<T>(&instance, key)
    }

    fn lookup(&self, key: &ProviderKey) -> Result<usize, KernelError> {
        let graph = &self.inner.graph;
        match graph.lookup(self.module_index, key) {
            Some(Visible::One(index)) => Ok(*index),
            found => Err(graph.unresolved(
                found,
                self.module_index,
                self.module.name(),
                key,
                self.required_by(),
            )),
        }
    }

    /// What is asking for a provider, for error messages.
    fn required_by(&self) -> String {
        match self.stack.frames().next() {
            Some(frame) => format!("`{}`", self.key_of(frame.provider)),
            None => "a direct resolver call".to_owned(),
        }
    }

    fn key_of(&self, index: usize) -> &ProviderKey {
        &self.inner.graph.providers[index].def.key
    }

    /// Resolves a provider by index, honouring its scope.
    pub(crate) async fn resolve_index(&self, index: usize) -> Result<Instance, KernelError> {
        let node = &self.inner.graph.providers[index];
        let singleton_cell = &self.inner.singletons[index];
        if node.def.scope == Scope::Singleton
            && let Some(instance) = singleton_cell.get()
        {
            return Ok(instance.clone());
        }
        // Checked before touching any cell: re-entering a cell that is being
        // initialised by this very resolution chain would deadlock.
        if let Some(path) = self.stack.cycle_through(index) {
            return Err(KernelError::ProviderCycle {
                path: path
                    .into_iter()
                    .map(|p| self.key_of(p).to_string())
                    .collect(),
            });
        }
        match node.def.scope {
            Scope::Singleton => {
                // Singletons never see the request scope, so they cannot capture request state.
                let child = self.child(index, None);
                singleton_cell
                    .get_or_try_init(|| (node.def.factory)(child))
                    .await
                    .cloned()
            }
            Scope::Request => {
                if let Some(singleton) = self.stack.frames().find(|f| self.is_singleton(f.provider))
                {
                    let graph = &self.inner.graph;
                    return Err(KernelError::ScopeInversion {
                        module: graph.modules[graph.providers[singleton.provider].module].name,
                        singleton: self.key_of(singleton.provider).to_string(),
                        request_scoped: node.def.key.to_string(),
                    });
                }
                let Some(scope) = &self.scope else {
                    return Err(KernelError::MissingRequestScope {
                        provider: node.def.key.to_string(),
                    });
                };
                let cell = scope.cell(app_identity(&self.inner), index);
                let child = self.child(index, Some(scope.clone()));
                cell.get_or_try_init(|| (node.def.factory)(child))
                    .await
                    .cloned()
            }
            Scope::Transient => (node.def.factory)(self.child(index, self.scope.clone())).await,
        }
    }

    fn is_singleton(&self, index: usize) -> bool {
        self.inner.graph.providers[index].def.scope == Scope::Singleton
    }

    /// The resolver handed to the factory of provider `index`: the owning
    /// module's visibility, one more frame on the stack.
    fn child(&self, index: usize, scope: Option<RequestScope>) -> Resolver {
        let owner = self.inner.graph.providers[index].module;
        Resolver {
            inner: self.inner.clone(),
            module: self.inner.graph.modules[owner].id,
            module_index: Some(owner),
            scope,
            stack: self.stack.push(index),
        }
    }
}

impl fmt::Debug for Resolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolver")
            .field("module", &self.module.name())
            .field("request_scoped", &self.scope.is_some())
            .finish_non_exhaustive()
    }
}

/// Extracts the `Arc<T>` stored in an instance.
fn downcast<T: ?Sized + Send + Sync + 'static>(
    instance: &Instance,
    key: &ProviderKey,
) -> Result<Arc<T>, KernelError> {
    instance
        .downcast_ref::<Arc<T>>()
        .cloned()
        .ok_or_else(|| KernelError::Construction {
            provider: key.to_string(),
            source: format!(
                "internal error: instance is not an `Arc<{}>`",
                std::any::type_name::<T>()
            )
            .into(),
        })
}

/// The providers currently being constructed, innermost first (a persistent list).
#[derive(Clone, Default)]
struct Stack(Option<Arc<Frame>>);

struct Frame {
    provider: usize,
    parent: Option<Arc<Frame>>,
}

impl Stack {
    fn push(&self, provider: usize) -> Stack {
        Stack(Some(Arc::new(Frame {
            provider,
            parent: self.0.clone(),
        })))
    }

    fn frames(&self) -> impl Iterator<Item = &Frame> {
        std::iter::successors(self.0.as_deref(), |frame| frame.parent.as_deref())
    }

    /// If `provider` is already being constructed, the cycle from its first
    /// construction back to itself (`[A, B, A]`).
    fn cycle_through(&self, provider: usize) -> Option<Vec<usize>> {
        let mut path: Vec<usize> = Vec::new();
        for frame in self.frames() {
            path.push(frame.provider);
            if frame.provider == provider {
                path.reverse();
                path.push(provider);
                return Some(path);
            }
        }
        None
    }
}
