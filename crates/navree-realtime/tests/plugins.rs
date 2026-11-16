//! Channel authorizers a plugin registers: under the plugin's namespace only. The app authorizes
//! any channel with a plugin's function from a module of its own.

#[path = "realtime/support.rs"]
mod support;

use http::StatusCode;
use navree_kernel::{Module, ModuleBuilder};
use navree_realtime::ModuleBuilderRealtimeExt;
use uuid::Uuid;

use navree_plugin_notice_board::BoardModule;
use support::{Server, signature};

/// The socket the auth requests sign for.
const SOCKET: &str = "1234.5678";

/// The notice board plugin, a community plugin whose crate is this module: the test app's own
/// modules are in no crate it names.
mod navree_plugin_notice_board {
    use std::collections::HashMap;

    use navree_contracts::Principal;
    use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder, PluginInfo};
    use navree_realtime::{ChannelDecision, ModuleBuilderRealtimeExt};

    /// The plugin's info.
    const NOTICE_BOARD: PluginInfo = PluginInfo::new(
        "navree-plugin-notice-board",
        module_path!(),
        "notice_board",
        "1.0.0",
    );

    /// The plugin's channel authorizer: every signed-in user may listen.
    pub fn anyone(
        _principal: &Principal,
        _params: &HashMap<String, String>,
        _ctx: &AppContext,
    ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
        Box::pin(async { Ok(ChannelDecision::Allow) })
    }

    /// The plugin's module: it names the plugin and authorizes the channels of the pattern
    /// `.0` with [`anyone`].
    pub struct BoardModule(pub &'static str);

    impl Module for BoardModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(NOTICE_BOARD).channel(self.0, anyone);
        }
    }
}

/// A module of the app: it authorizes `orders.{id}` with the notice board's function.
struct OrdersModule;

impl Module for OrdersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("orders.{id}", navree_plugin_notice_board::anyone);
    }
}

/// The refusal of the notice board's `BoardModule` authorizing `pattern`.
fn refusal(pattern: &str) -> String {
    format!(
        "channel pattern `{pattern}` of module `BoardModule`: plugin `navree-plugin-notice-board` authorizes channels under `notice_board.` only, and only a module of the app authorizes the app's other channels: remove the plugin or update it to a version that names its channels under `notice_board.`, and to use the plugin's authorizer for `{pattern}`, register it from a module of the app (m.channel)"
    )
}

#[tokio::test]
async fn a_plugin_channel_outside_its_namespace_fails_the_boot() {
    // `{scope}.{id}` would match `private-users.{someone}`, the channel of each of the app's
    // users; `notice_board_extra.` and a namespace after a parameter are not under it either.
    for pattern in [
        "{scope}.{id}",
        "users.{id}",
        "notice_board_extra.{id}",
        "{board}.notice_board",
    ] {
        let error = support::app(support::options())
            .module(BoardModule(pattern))
            .boot()
            .await
            .expect_err("the boot must fail")
            .to_string();
        assert!(error.contains(&refusal(pattern)), "{pattern}: {error}");
    }

    // Under its namespace, the plugin's authorizer decides its own channels.
    let server = Server::serve(
        support::app(support::options()).module(BoardModule("notice_board.{id}")),
    )
    .await;
    let channel = "private-notice_board.7";
    let reply = server.auth(Some(Uuid::now_v7()), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.json["auth"], signature(SOCKET, channel, None));
}

#[tokio::test]
async fn the_app_may_authorize_any_channel_with_a_plugins_function() {
    let server = Server::serve(
        support::app(support::options())
            .module(BoardModule("notice_board.{id}"))
            .module(OrdersModule),
    )
    .await;
    let channel = "private-orders.7";
    let reply = server.auth(Some(Uuid::now_v7()), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.json["auth"], signature(SOCKET, channel, None));

    // The module that authorizes `orders.{id}` is the app's: the plugin lists its own only.
    let plugins = server.app.ctx().plugins().plugins;
    assert_eq!(plugins.len(), 1);
    let modules: Vec<&str> = plugins[0]
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert_eq!(modules, ["BoardModule"]);
}
