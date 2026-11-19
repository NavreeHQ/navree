//! The [`Module`] and [`Injectable`] traits, and [`ConfiguredModule`].

use std::marker::PhantomData;

use crate::builder::PassedModule;
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

    /// The concrete type name of `self` when erased behind `dyn Module`.
    /// Sealed by an unnameable parameter so crates outside `navree-kernel`
    /// cannot override it. Not public API.
    #[doc(hidden)]
    #[allow(private_interfaces)]
    fn __module_type(&self, _: sealed::Witness<'_>) -> sealed::ModuleType {
        sealed::ModuleType {
            type_name: std::any::type_name::<Self>(),
        }
    }
}

mod sealed {
    use std::marker::PhantomData;

    #[derive(Clone, Copy)]
    pub struct Witness<'a>(PhantomData<fn(&'a ()) -> &'a ()>);

    impl Witness<'_> {
        pub(super) fn new() -> Self {
            Self(PhantomData)
        }
    }

    #[derive(Clone, Copy)]
    pub struct ModuleType {
        pub(super) type_name: &'static str,
    }
}

impl Module for Box<dyn Module> {
    fn id(&self) -> ModuleId {
        (**self).id()
    }

    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn is_configured(&self) -> bool {
        (**self).is_configured()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        (**self).register(m);
    }

    #[allow(private_interfaces)]
    fn __module_type(&self, witness: sealed::Witness<'_>) -> sealed::ModuleType {
        (**self).__module_type(witness)
    }
}

impl Module for std::pin::Pin<Box<dyn Module>> {
    fn id(&self) -> ModuleId {
        self.as_ref().get_ref().id()
    }

    fn name(&self) -> &'static str {
        self.as_ref().get_ref().name()
    }

    fn is_configured(&self) -> bool {
        self.as_ref().get_ref().is_configured()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        self.as_ref().get_ref().register(m);
    }

    #[allow(private_interfaces)]
    fn __module_type(&self, witness: sealed::Witness<'_>) -> sealed::ModuleType {
        self.as_ref().get_ref().__module_type(witness)
    }
}

/// Unwraps erased `dyn Module` wrappers (`Box`, `Pin`) and returns the inner
/// value's `std::any::type_name`.
pub(crate) fn resolved_type_name(module: &dyn Module) -> &'static str {
    module.__module_type(sealed::Witness::new()).type_name
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
/// A module of a plugin that is not one of the framework's own packages may
/// configure only modules whose type names the plugin's crate; it imports any
/// other module by type, and the app configures it.
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
/// // The app's root module and GreetingModule.
/// assert_eq!(report.modules.len(), 2);
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
        if self.options.is_some() {
            m.contribute(CodeConfigured {
                module: ModuleId::of::<M>().name(),
            });
        }
        (self.register)(m, self.options.clone());
    }
}

/// Expands, in the crate that calls it, to a configured-module wrapper type of
/// that crate with one private field holding [`ConfiguredModule`], two
/// crate-private constructors (`for_root(options)` and `from_config()`) bound
/// to the crate's own `register` function, and [`Module`] forwarding `id`,
/// `is_configured` and `register`.
///
/// A framework package that names itself a plugin uses this in place of
/// returning [`ConfiguredModule`] directly, so its configured forms have a type
/// of the package's own crate and no other crate can build the wrapper with
/// another `register` function. Not public API; Navree's own crates use it.
///
/// ```
/// use navree_kernel::{Module, ModuleBuilder, Navree, configured_module};
///
/// #[derive(Clone)]
/// struct GreetingOptions(&'static str);
///
/// struct GreetingModule;
///
/// impl GreetingModule {
///     pub fn for_root(options: GreetingOptions) -> impl Module {
///         ConfiguredGreetingModule::for_root(options)
///     }
///
///     pub fn from_config() -> impl Module {
///         ConfiguredGreetingModule::from_config()
///     }
/// }
///
/// impl Module for GreetingModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         register(m, None);
///     }
/// }
///
/// configured_module!(ConfiguredGreetingModule, GreetingModule, GreetingOptions, register);
///
/// fn register(m: &mut ModuleBuilder, options: Option<GreetingOptions>) {
///     let greeting = options.map_or("sat sri akal", |options| options.0);
///     m.value(greeting.to_owned()).exports::<String>();
/// }
///
/// let configured = GreetingModule::for_root(GreetingOptions("namaste"));
/// assert_eq!(configured.id(), GreetingModule.id());
/// assert!(configured.is_configured());
/// Navree::app().module(configured).validate().expect("valid");
/// ```
///
/// Outside the crate that wrote a configured form, neither its constructors
/// nor its private field can build one:
///
/// ```compile_fail
/// use navree_kernel::__private::ConfiguredPlainModule;
///
/// let _ = ConfiguredPlainModule::from_config();
/// ```
///
/// ```compile_fail
/// use navree_kernel::ConfiguredModule;
/// use navree_kernel::__private::{ConfiguredPlainModule, PlainModule};
///
/// let _ = ConfiguredPlainModule(ConfiguredModule::<PlainModule, ()>::from_config(|_, _| {}));
/// ```
#[doc(hidden)]
#[macro_export]
macro_rules! configured_module {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident,
        $module:ty,
        $options:ty,
        $register:expr $(,)?
    ) => {
        #[allow(missing_docs)]
        $(#[$meta])*
        $vis struct $name($crate::ConfiguredModule<$module, $options>);

        impl $name {
            #[allow(dead_code)]
            pub(crate) fn for_root(options: $options) -> Self {
                Self($crate::ConfiguredModule::for_root(options, $register))
            }

            #[allow(dead_code)]
            pub(crate) fn from_config() -> Self {
                Self($crate::ConfiguredModule::from_config($register))
            }
        }

        impl $crate::Module for $name {
            fn id(&self) -> $crate::ModuleId {
                $crate::Module::id(&self.0)
            }

            fn is_configured(&self) -> bool {
                $crate::Module::is_configured(&self.0)
            }

            fn register(&self, m: &mut $crate::ModuleBuilder) {
                $crate::Module::register(&self.0, m);
            }
        }
    };
}

mod sample {
    /// A module of the kernel that registers nothing, for the doctests of
    /// [`configured_module!`](crate::configured_module) and the plugin tests.
    /// Not public API.
    ///
    /// Outside `navree-kernel`, `Module::__module_type` cannot be overridden:
    ///
    /// ```compile_fail
    /// use navree_kernel::{Module, ModuleBuilder};
    ///
    /// struct OutsideModule;
    ///
    /// impl Module for OutsideModule {
    ///     fn register(&self, _: &mut ModuleBuilder) {}
    ///
    ///     fn __module_type(&self, _: _) -> _ {
    ///         unreachable!()
    ///     }
    /// }
    /// ```
    pub struct PlainModule;

    impl PlainModule {
        /// Configured form of [`PlainModule`], for plugin tests.
        pub fn from_config() -> ConfiguredPlainModule {
            ConfiguredPlainModule::from_config()
        }
    }

    impl crate::Module for PlainModule {
        fn register(&self, _: &mut crate::ModuleBuilder) {}
    }

    fn register(_: &mut crate::ModuleBuilder, _: Option<()>) {}

    crate::configured_module!(pub ConfiguredPlainModule, PlainModule, (), register);
}

pub use sample::{ConfiguredPlainModule, PlainModule};

/// Contributed by a module that was given its options in code
/// (`for_root(options)`), so tools can tell that the section of the
/// configuration it would otherwise read is not read. navree-config warns
/// when the configuration has such a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeConfigured {
    module: &'static str,
}

impl CodeConfigured {
    /// The short name of the module (`CacheModule`).
    pub fn module(&self) -> &'static str {
        self.module
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

    /// Converts into modules, in order, each with the type of the value it was
    /// given as: a module given already boxed has the type `Box<dyn Module>`.
    /// The kernel reads a module's own type from it, and only a typed value
    /// makes a `PassedModule`. Not public API; the kernel calls it.
    #[doc(hidden)]
    fn into_passed_modules(self) -> Vec<PassedModule>
    where
        Self: Sized,
    {
        self.into_modules()
            .into_iter()
            .map(PassedModule::boxed)
            .collect()
    }
}

impl<M: Module> IntoModules for M {
    fn into_modules(self) -> Vec<Box<dyn Module>> {
        vec![Box::new(self)]
    }

    fn into_passed_modules(self) -> Vec<PassedModule> {
        vec![PassedModule::of(self)]
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

            #[allow(non_snake_case)]
            fn into_passed_modules(self) -> Vec<PassedModule> {
                let ($($name,)+) = self;
                vec![$(PassedModule::of($name)),+]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Navree;
    use crate::graph::Graph;
    use crate::plugin::{FIRST_PARTY, PluginInfo, is_first_party};

    /// Stands for a framework package's module and its configured form written in that crate.
    mod navree_flags_probe {
        use crate::{Module, ModuleBuilder};

        #[derive(Clone)]
        pub struct FlagsOptions(pub &'static str);

        pub struct FlagsModule;

        impl FlagsModule {
            pub fn for_root(options: FlagsOptions) -> impl Module {
                ConfiguredFlagsModule::for_root(options)
            }

            pub fn from_config() -> impl Module {
                ConfiguredFlagsModule::from_config()
            }
        }

        impl Module for FlagsModule {
            fn register(&self, m: &mut ModuleBuilder) {
                register(m, None);
            }
        }

        crate::configured_module!(ConfiguredFlagsModule, FlagsModule, FlagsOptions, register);

        fn register(m: &mut ModuleBuilder, options: Option<FlagsOptions>) {
            let city = options.map_or("from_config", |options| options.0);
            m.contribute(city);
        }
    }

    #[test]
    fn a_configured_form_written_in_a_crate_is_a_type_of_that_crate_with_the_modules_id() {
        use navree_flags_probe::{FlagsModule, FlagsOptions};

        let plain_id = ModuleId::of::<FlagsModule>();
        let krate = concat!(module_path!(), "::navree_flags_probe");
        let prefix = format!("{krate}::");

        let for_root = FlagsModule::for_root(FlagsOptions("Ludhiana"));
        assert_eq!(for_root.id(), plain_id);
        assert_eq!(for_root.name(), "FlagsModule");
        assert!(for_root.is_configured());
        let passed_root = PassedModule::of(for_root);
        assert!(
            passed_root.type_name.starts_with(prefix.as_str()),
            "{}",
            passed_root.type_name
        );
        assert!(
            !passed_root.type_name.contains('<'),
            "{}",
            passed_root.type_name
        );

        let graph = Graph::build_passed(&[passed_root], &[]).expect("valid");
        let m = graph.module_of(plain_id).expect("in the graph");
        let code_configured: Vec<&str> = graph.modules[m]
            .contributions
            .iter()
            .filter_map(|c| c.value.downcast_ref::<CodeConfigured>())
            .map(|c| c.module())
            .collect();
        assert_eq!(code_configured, ["FlagsModule"]);
        let cities: Vec<&str> = graph.modules[m]
            .contributions
            .iter()
            .filter_map(|c| c.value.downcast_ref::<&'static str>().copied())
            .collect();
        assert_eq!(cities, ["Ludhiana"]);

        let from_config = FlagsModule::from_config();
        assert_eq!(from_config.id(), plain_id);
        assert!(from_config.is_configured());
        let passed_config = PassedModule::of(from_config);
        assert!(
            passed_config.type_name.starts_with(prefix.as_str()),
            "{}",
            passed_config.type_name
        );
        let listed = [("navree-flags", krate)];
        let info = PluginInfo::new("navree-flags", krate, "flags", "1.4.0");
        assert!(is_first_party(
            &info,
            &[passed_config.type_name],
            &listed,
            "1.4.0"
        ));

        let graph = Graph::build_passed(&[passed_config], &[]).expect("valid");
        let m = graph.module_of(plain_id).expect("in the graph");
        let has_code_configured = graph.modules[m]
            .contributions
            .iter()
            .any(|c| c.value.downcast_ref::<CodeConfigured>().is_some());
        assert!(!has_code_configured);
        let cities: Vec<&str> = graph.modules[m]
            .contributions
            .iter()
            .filter_map(|c| c.value.downcast_ref::<&'static str>().copied())
            .collect();
        assert_eq!(cities, ["from_config"]);
    }

    #[test]
    fn the_kernels_own_configured_wrapper_never_makes_a_plugin_first_party() {
        use navree_flags_probe::{FlagsModule, FlagsOptions};

        let version = env!("CARGO_PKG_VERSION");
        let krate = concat!(module_path!(), "::navree_flags_probe");
        let listed = [("navree-flags", krate)];
        let info = PluginInfo::new("navree-flags", krate, "flags", version);

        let for_root = PassedModule::of(ConfiguredModule::<FlagsModule, FlagsOptions>::for_root(
            FlagsOptions("Ludhiana"),
            |_, _| {},
        ));
        let from_config = PassedModule::of(
            ConfiguredModule::<FlagsModule, FlagsOptions>::from_config(|_, _| {}),
        );
        for wrapped in [&for_root, &from_config] {
            assert!(
                !is_first_party(&info, &[wrapped.type_name], &listed, version),
                "{}",
                wrapped.type_name
            );
        }

        // Even when the module and options inside `ConfiguredModule` are types of the listed
        // first-party crate, the wrapper's own outer type is the kernel's: not first-party, and
        // refused in the reserved namespace.
        let flags = PluginInfo::new("navree-flags", "navree_flags", "flags", version);
        let kernel_wrapped = "navree_kernel::module::ConfiguredModule<navree_flags::module::FlagsModule, navree_flags::options::FlagsOptions>";
        assert!(!is_first_party(
            &flags,
            &[kernel_wrapped],
            FIRST_PARTY,
            version
        ));

        let mut graph =
            Graph::build_passed(&[PassedModule::of(PlainModule)], &[]).expect("a plain module");
        let m = graph
            .module_of(ModuleId::of::<PlainModule>())
            .expect("in the graph");
        graph.modules[m].type_name = kernel_wrapped;
        graph.modules[m].plugin = Some(flags);
        let mut errors = Vec::new();
        graph.check_plugins(&mut errors);
        assert!(
            matches!(
                errors.as_slice(),
                [KernelError::NamespaceReserved {
                    plugin: "navree-flags",
                    namespace: "flags",
                }]
            ),
            "{errors:?}"
        );
        assert!(!graph.first_party.contains("navree-flags"));

        let claimed = ConfiguredModule::<FlagsModule, ()>::from_config(|m, _| {
            m.plugin(PluginInfo::new(
                "navree-flags",
                "navree_kernel",
                "flags",
                env!("CARGO_PKG_VERSION"),
            ));
        });
        let refused = Navree::app()
            .module(claimed)
            .validate()
            .expect_err("the kernel's own wrapper is never first-party");
        assert!(
            matches!(
                refused.0.as_slice(),
                [KernelError::NamespaceReserved {
                    plugin: "navree-flags",
                    namespace: "flags",
                }]
            ),
            "{refused:?}"
        );
    }
}
