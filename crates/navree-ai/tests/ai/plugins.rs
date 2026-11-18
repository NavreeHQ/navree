//! Agent tools whose access attributes name a community plugin's namespace.

use navree_access::{AccessModule, Subject};
use navree_ai::{Agent, AgentRun, ModuleBuilderAiExt, ToolAccess, ToolSpec};
use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, PluginInfo, Resolver,
};
use serde_json::json;

use crate::support::agent_app;

mod navree_plugin_khata {
    use super::*;

    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    pub struct Ledger;

    impl Subject for Ledger {
        const SUBJECT: &'static str = "khata.Ledger";
        const PERMISSION_PREFIX: &'static str = "khata.ledgers";
    }

    pub struct KhataPlugin {
        pub describe_permission: bool,
        pub register_subject: bool,
    }

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            if self.describe_permission {
                m.permissions(&[Permission::new("khata.close", "Close a khata ledger")]);
            }
            if self.register_subject {
                m.import(AccessModule::for_subject::<Ledger>());
            }
        }
    }
}

struct CloseLedgerAssistant;

impl Injectable for CloseLedgerAssistant {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Agent for CloseLedgerAssistant {
    const NAME: &'static str = "khata-assistant";

    fn instructions(&self, _run: &AgentRun) -> String {
        "Assist Simran Kaur with Ludhiana ledgers.".to_owned()
    }

    fn tool_specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec::new("close_ledger", "Close a ledger", json!({"type": "object"}))
                .access(ToolAccess::permissions(&["khata.close"])),
        ]
    }
}

struct ReadLedgerAssistant;

impl Injectable for ReadLedgerAssistant {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Agent for ReadLedgerAssistant {
    const NAME: &'static str = "ledger-assistant";

    fn instructions(&self, _run: &AgentRun) -> String {
        "Assist Gurpreet Singh with Amritsar ledgers.".to_owned()
    }

    fn tool_specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec::new("list_ledgers", "List ledgers", json!({"type": "object"}))
                .access(ToolAccess::can("read", "khata.Ledger")),
        ]
    }
}

struct AppCloseAgent;

impl Module for AppCloseAgent {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<CloseLedgerAssistant>();
    }
}

struct AppReadAgent;

impl Module for AppReadAgent {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<ReadLedgerAssistant>();
    }
}

#[tokio::test]
async fn an_agent_tool_that_requires_a_permission_under_a_plugins_namespace_the_plugin_does_not_describe_fails_the_boot()
 {
    use navree_plugin_khata::KhataPlugin;

    // 1. Permission under `khata` not described by the plugin: boot fails naming the agent tool.
    let err = match agent_app()
        .module(KhataPlugin {
            describe_permission: false,
            register_subject: false,
        })
        .module(AppCloseAgent)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when the plugin does not describe `khata.close`"),
        Err(error) => error.to_string(),
    };
    let expected_perm = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.close` that the agent tool `khata-assistant.close_ledger` of `AppCloseAgent` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(
        err.contains(expected_perm),
        "expected `{expected_perm}` in:\n{err}"
    );

    // 2. Subject under `khata` not registered by the plugin: boot fails naming the agent tool.
    let err = match agent_app()
        .module(KhataPlugin {
            describe_permission: false,
            register_subject: false,
        })
        .module(AppReadAgent)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when the plugin does not register `khata.Ledger`"),
        Err(error) => error.to_string(),
    };
    let expected_subject = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the subject `khata.Ledger` that the agent tool `ledger-assistant.list_ledgers` of `AppReadAgent` checks, and the plugin registers no subject of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: check a subject the plugin registers, or name the app's subject under another word";
    assert!(
        err.contains(expected_subject),
        "expected `{expected_subject}` in:\n{err}"
    );

    // 3. When the plugin describes the permission and registers the subject, both boot.
    let app = agent_app()
        .module(KhataPlugin {
            describe_permission: true,
            register_subject: true,
        })
        .module(AppCloseAgent)
        .module(AppReadAgent)
        .build()
        .await;
    app.shutdown().await.unwrap();
}
