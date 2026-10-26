//! The [`Module`] and [`Injectable`] traits.

use crate::{BoxFuture, KernelError, ModuleBuilder, ModuleId, ProviderKey, Resolver, Scope};

/// A unit of an app: it registers providers, exports, imports, contributions,
/// lifecycle hooks and commands through a [`ModuleBuilder`].
///
/// `register` is called once per [`ModuleId`] per boot. A configured instance
/// (`for_root(...)`, [`is_configured`](Module::is_configured) returns `true`)
/// wins over plain imports of the same module type.
pub trait Module: Send + Sync + 'static {
    /// The module identity. `for_root` wrappers override this to return the id
    /// of the module type they configure.
    fn id(&self) -> ModuleId {
        ModuleId::of::<Self>()
    }

    /// The readable module name used in errors and reports.
    fn name(&self) -> &'static str {
        self.id().name()
    }

    /// `for_root(...)` values return true; at most one configured instance per id.
    fn is_configured(&self) -> bool {
        false
    }

    /// Registers everything the module contributes.
    fn register(&self, m: &mut ModuleBuilder);
}

/// A type the container can construct (what `#[injectable]` implements).
pub trait Injectable: Sized + Send + Sync + 'static {
    /// The provider scope; singletons by default.
    fn scope() -> Scope {
        Scope::Singleton
    }

    /// The keys `construct` resolves, used by the boot-time graph check.
    /// Keys resolved through [`Lazy`](crate::Lazy) are left out.
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    /// Builds the value, resolving dependencies through `r`.
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>>;
}

/// Values accepted by [`ModuleBuilder::imports`]: a module, a tuple of up to
/// 16 modules, or a `Vec<Box<dyn Module>>`.
pub trait IntoModules {
    /// Converts into boxed modules, in order.
    fn into_modules(self) -> Vec<Box<dyn Module>>;
}

impl<M: Module> IntoModules for M {
    fn into_modules(self) -> Vec<Box<dyn Module>> {
        vec![Box::new(self)]
    }
}

impl IntoModules for Vec<Box<dyn Module>> {
    fn into_modules(self) -> Vec<Box<dyn Module>> {
        self
    }
}

macro_rules! tuple_into_modules {
    ($($name:ident),+) => {
        impl<$($name: Module),+> IntoModules for ($($name,)+) {
            #[allow(non_snake_case)]
            fn into_modules(self) -> Vec<Box<dyn Module>> {
                let ($($name,)+) = self;
                vec![$(Box::new($name) as Box<dyn Module>),+]
            }
        }
    };
}

tuple_into_modules!(A);
tuple_into_modules!(A, B);
tuple_into_modules!(A, B, C);
tuple_into_modules!(A, B, C, D);
tuple_into_modules!(A, B, C, D, E);
tuple_into_modules!(A, B, C, D, E, F);
tuple_into_modules!(A, B, C, D, E, F, G);
tuple_into_modules!(A, B, C, D, E, F, G, H);
tuple_into_modules!(A, B, C, D, E, F, G, H, I);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K, L);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K, L, M);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_into_modules!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
