//! Configured (`for_root`) modules versus plain imports, and module dedupe.

use std::sync::Arc;

use navis_kernel::{KernelError, Module, ModuleBuilder, ModuleId, Navis};

use crate::support::{Counter, boot_errors, injectable, module};

/// A configurable module: plain `Settings` registers defaults,
/// `Settings::for_root(..)` registers the given value.
pub struct Settings;

impl Settings {
    pub fn for_root(value: &'static str) -> ConfiguredSettings {
        ConfiguredSettings(value)
    }
}

impl Module for Settings {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(String::from("default")).exports::<String>();
    }
}

pub struct ConfiguredSettings(&'static str);

impl Module for ConfiguredSettings {
    fn id(&self) -> ModuleId {
        ModuleId::of::<Settings>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.value(String::from(self.0)).exports::<String>();
    }
}

injectable!(Greeter { greeting: String });

module!(GreeterModule, |m| {
    m.import(Settings)
        .provider::<Greeter>()
        .exports::<Greeter>();
});

module!(TwiceConfigured, |m| {
    m.imports((Settings::for_root("a"), Settings::for_root("b")));
});

#[tokio::test]
async fn rejects_module_configured_twice() {
    let errors = boot_errors(TwiceConfigured).await;
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::ConfiguredTwice { module: "Settings" }]
        ),
        "{errors:?}"
    );
}

#[tokio::test]
async fn configured_instance_wins_over_plain_import() {
    let app = Navis::app()
        .module(GreeterModule)
        .module(Settings::for_root("configured"))
        .boot()
        .await
        .expect("boot");
    let greeter = app.ctx().get::<Greeter>().await.expect("Greeter");
    assert_eq!(greeter.greeting.as_str(), "configured");
}

module!(LateConfigurer, |m| {
    m.import(Settings::for_root("late"));
});

module!(EarlyPlainImporter, |m| {
    m.imports((Settings, LateConfigurer)).provider::<Greeter>();
});

#[tokio::test]
async fn configured_instance_found_after_plain_import_still_wins() {
    let app = Navis::app()
        .module(EarlyPlainImporter)
        .boot()
        .await
        .expect("boot");
    let greeter = app.ctx().get::<Greeter>().await.expect("Greeter");
    assert_eq!(greeter.greeting.as_str(), "late");
}

/// A plain module whose `register` calls are counted.
pub struct Shared(Arc<Counter>);

impl Module for Shared {
    fn register(&self, m: &mut ModuleBuilder) {
        self.0.hit();
        m.value(7_u32).exports::<u32>();
    }
}

pub struct FirstUser(Arc<Counter>);
pub struct SecondUser(Arc<Counter>);

impl Module for FirstUser {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(Shared(self.0.clone()));
    }
}

impl Module for SecondUser {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(Shared(self.0.clone()));
    }
}

#[tokio::test]
async fn dedupes_plain_module_imported_twice() {
    let counter = Arc::new(Counter::default());
    let app = Navis::app()
        .module(FirstUser(counter.clone()))
        .module(SecondUser(counter.clone()))
        .boot()
        .await
        .expect("boot");
    assert_eq!(counter.get(), 1, "Shared::register runs once");
    assert_eq!(*app.ctx().get::<u32>().await.expect("u32"), 7);
}
