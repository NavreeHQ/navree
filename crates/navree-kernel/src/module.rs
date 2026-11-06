//! The [`Module`] and [`Injectable`] traits, and [`ConfiguredModule`].

use std::marker::PhantomData;

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

/// The configured form of module `M`: what `M::for_root(options)` and
/// `M::from_config()` return, so a module does not repeat the wrapper that
/// carries its id, says it is configured and hands its options to `register`.
///
/// `register` is the module's own function. It gets `Some(options)` from
/// [`for_root`](ConfiguredModule::for_root), and `None` from
/// [`from_config`](ConfiguredModule::from_config), which means "read the
/// section of the app's configuration" (a plain `M` registers with `None`
/// too).
///
/// ```
/// use navree_kernel::{ConfiguredModule, Module, ModuleBuilder, Navree};
///
/// #[derive(Clone)]
/// struct GreetingOptions(&'static str);
///
/// struct GreetingModule;
///
/// impl GreetingModule {
///     pub fn for_root(options: GreetingOptions) -> impl Module {
///         ConfiguredModule::<GreetingModule, _>::for_root(options, register)
///     }
/// }
///
/// impl Module for GreetingModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         register(m, None);
///     }
/// }
///
/// fn register(m: &mut ModuleBuilder, options: Option<GreetingOptions>) {
///     let greeting = options.map_or("hello", |options| options.0);
///     m.value(greeting.to_owned()).exports::<String>();
/// }
///
/// // The configured instance has the plain module's id.
/// let configured = GreetingModule::for_root(GreetingOptions("namaste"));
/// assert_eq!(configured.id(), GreetingModule.id());
/// assert!(configured.is_configured());
/// let report = Navree::app().module(configured).validate().expect("valid");
/// assert_eq!(report.modules.len(), 1);
/// ```
pub struct ConfiguredModule<M, O> {
    options: Option<O>,
    register: fn(&mut ModuleBuilder, Option<O>),
    module: PhantomData<fn() -> M>,
}

impl<M, O> ConfiguredModule<M, O> {
    /// `M` configured with `options` (configure a module once, in the app
    /// root).
    pub fn for_root(options: O, register: fn(&mut ModuleBuilder, Option<O>)) -> Self {
        Self {
            options: Some(options),
            register,
            module: PhantomData,
        }
    }

    /// `M` configured from the app's configuration: `register` gets `None`.
    pub fn from_config(register: fn(&mut ModuleBuilder, Option<O>)) -> Self {
        Self {
            options: None,
            register,
            module: PhantomData,
        }
    }
}

impl<M: 'static, O: Clone + Send + Sync + 'static> Module for ConfiguredModule<M, O> {
    fn id(&self) -> ModuleId {
        ModuleId::of::<M>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        (self.register)(m, self.options.clone());
    }
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
