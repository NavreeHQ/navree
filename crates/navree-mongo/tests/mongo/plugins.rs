//! An observer of every MongoDB commit: only a module of the app registers one. Needs no
//! server.

use navree_kernel::{Module, ModuleBuilder, Navree};
use navree_mongo::{ModuleBuilderMongoExt, MongoChangeObserver};

use navree_plugin_crm::{CrmModule, Tally};

/// The CRM plugin, a community plugin whose crate is this module: the app's modules beside it
/// are in no crate it names.
mod navree_plugin_crm {
    use navree_db::ChangeEvent;
    use navree_kernel::{
        BoxFuture, Injectable, KernelError, Module, ModuleBuilder, PluginInfo, Resolver,
    };
    use navree_mongo::{MongoChangeObserver, MongoResult, MongoUow};

    /// The plugin's info.
    const CRM: PluginInfo = PluginInfo::new("navree-plugin-crm", module_path!(), "crm", "1.0.0");

    /// An observer of every MongoDB commit that would copy each change of the Amritsar office
    /// to the plugin's ledger; here it changes nothing.
    pub struct Tally;

    impl Injectable for Tally {
        fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(Tally) })
        }
    }

    impl MongoChangeObserver for Tally {
        fn observe<'a>(
            &'a self,
            _uow: &'a MongoUow,
            _changes: &'a [ChangeEvent],
        ) -> BoxFuture<'a, MongoResult> {
            Box::pin(async { Ok(()) })
        }
    }

    /// The plugin's module: it names the plugin, then registers what `.0` adds.
    pub struct CrmModule(pub fn(&mut ModuleBuilder));

    impl Module for CrmModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(CRM);
            (self.0)(m);
        }
    }
}

/// A module of the app: it registers the CRM plugin's observer of every MongoDB commit.
struct Observers;

impl Module for Observers {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mongo_change_observer::<Tally>();
    }
}

#[test]
fn a_plugin_may_not_observe_every_mongodb_commit() {
    // The observer sees each commit's changes inside its transaction, writes in it, and its
    // error rolls the commit back.
    let refused = Navree::app()
        .module(CrmModule(|m| {
            m.mongo_change_observer::<Tally>();
        }))
        .validate()
        .expect_err("the plugin observes every commit")
        .to_string();
    let provider = std::any::type_name::<dyn MongoChangeObserver>();
    assert_eq!(
        refused,
        format!(
            "plugin `navree-plugin-crm` provides `{provider}` in `CrmModule`, which sees, and can change, every MongoDB write the app commits; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and to use the plugin's, register it from a module of the app"
        )
    );
    // A module of the app registers the plugin's observer beside the plugin.
    Navree::app()
        .module(CrmModule(|_| {}))
        .module(Observers)
        .validate()
        .expect("the app's registration of the plugin's observer");
}
