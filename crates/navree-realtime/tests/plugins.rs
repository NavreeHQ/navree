//! Channel authorizers a plugin registers: under the plugin's namespace only. The app authorizes
//! any channel with a plugin's function from a module of its own.

#[path = "realtime/support.rs"]
mod support;

use std::any::type_name;
use std::collections::HashMap;

use http::StatusCode;
use navree_contracts::Principal;
use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
use navree_realtime::{Backplane, ChannelDecision, ModuleBuilderRealtimeExt};
use uuid::Uuid;

use navree_plugin_chat::{ChatBackplane, ChatModule};
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
    pub const NOTICE_BOARD: PluginInfo = PluginInfo::new(
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

    /// A module of the plugin's crate that names no plugin itself.
    pub struct Helper(pub &'static str);

    impl Module for Helper {
        fn register(&self, m: &mut ModuleBuilder) {
            m.channel(self.0, anyone);
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
    // users; `notice_board_extra.`, a namespace after a parameter and the namespace alone are not
    // under it either.
    for pattern in [
        "{scope}.{id}",
        "users.{id}",
        "notice_board_extra.{id}",
        "{board}.notice_board",
        "notice_board",
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
    let server =
        Server::serve(support::app(support::options()).module(BoardModule("notice_board.{id}")))
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

/// The chat plugin, a community plugin whose crate is this module: its rooms are the channels
/// `chat.{room}`.
mod navree_plugin_chat {
    use std::collections::HashMap;
    use std::sync::Arc;

    use navree_contracts::Principal;
    use navree_kernel::{
        AppContext, BoxError, BoxFuture, Factory, Module, ModuleBuilder, PluginInfo,
    };
    use navree_realtime::{Backplane, ChannelDecision, MemoryBackplane, ModuleBuilderRealtimeExt};

    /// The plugin's info: its namespace is `chat`.
    const ROOMS: PluginInfo =
        PluginInfo::new("navree-plugin-chat", module_path!(), "chat", "1.0.0");

    /// The plugin's channel authorizer: every signed-in user may join a room.
    pub fn anyone(
        _principal: &Principal,
        _params: &HashMap<String, String>,
        _ctx: &AppContext,
    ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
        Box::pin(async { Ok(ChannelDecision::Allow) })
    }

    /// The plugin's module: it names the plugin and authorizes its rooms with [`anyone`].
    pub struct ChatModule;

    impl Module for ChatModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(ROOMS).channel("chat.{room}", anyone);
        }
    }

    /// The plugin's backplane, for a module of the app to register.
    pub fn backplane() -> Factory<dyn Backplane> {
        Factory::new(|_| async { Ok(Arc::new(MemoryBackplane::new()) as Arc<dyn Backplane>) })
    }

    /// A module of the plugin that registers its backplane: with `realtime.backplane = none`,
    /// the app's broadcasts would go through it.
    pub struct ChatBackplane;

    impl Module for ChatBackplane {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(ROOMS).factory(backplane());
        }
    }
}

/// The app's authorizer of its support desk: only its staff, and nobody here is.
fn staff_only(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Ok(ChannelDecision::Deny) })
}

/// A module of the app: it authorizes its support desk, `chat.support`, for its staff only.
struct SupportDesk;

impl Module for SupportDesk {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("chat.support", staff_only);
    }
}

/// A module of the app that imports the chat plugin's module, whose patterns then come first,
/// and authorizes `chat.support` for its staff only.
struct SupportDeskWithChat;

impl Module for SupportDeskWithChat {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(ChatModule).channel("chat.support", staff_only);
    }
}

/// A module of the app: its support desk is named outside the chat plugin's namespace,
/// `support.desk`, for its staff only.
struct SupportDeskOutside;

impl Module for SupportDeskOutside {
    fn register(&self, m: &mut ModuleBuilder) {
        m.channel("support.desk", staff_only);
    }
}

/// The refusal of the app's `module` authorizing `chat.support` beside the chat plugin.
fn under_chat(module: &str) -> String {
    format!(
        "channel pattern `chat.support` of module `{module}`: `chat` is the namespace of plugin `navree-plugin-chat`, whose authorizer would decide the channels named under it, and only the plugin's modules authorize channels there: name the channel outside the plugin's namespace, or leave that channel to the plugin's own pattern"
    )
}

#[tokio::test]
async fn an_app_channel_under_a_plugins_namespace_fails_the_boot() {
    // Beside the plugin, in either order: the plugin's `chat.{room}` would let anyone into the
    // app's support desk when it comes first.
    let beside = [
        support::app(support::options())
            .module(SupportDesk)
            .module(ChatModule),
        support::app(support::options())
            .module(ChatModule)
            .module(SupportDesk),
    ];
    for builder in beside {
        let error = builder
            .boot()
            .await
            .expect_err("the boot must fail")
            .to_string();
        let expected = under_chat("SupportDesk");
        assert!(error.contains(&expected), "{error}");
    }
    // The app's module imports the plugin's, so the plugin's pattern comes first.
    let error = support::app(support::options())
        .module(SupportDeskWithChat)
        .boot()
        .await
        .expect_err("the boot must fail")
        .to_string();
    let expected = under_chat("SupportDeskWithChat");
    assert!(error.contains(&expected), "{error}");

    // The plugin's own `chat.{room}` alone boots, and its authorizer decides its rooms.
    let server = Server::serve(support::app(support::options()).module(ChatModule)).await;
    let channel = "private-chat.7";
    let reply = server.auth(Some(Uuid::now_v7()), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.json["auth"], signature(SOCKET, channel, None));
}

#[tokio::test]
async fn the_two_ways_out_of_a_channel_under_a_plugins_namespace_boot() {
    // The app names its support desk outside the plugin's namespace: its own authorizer decides
    // it, in either order of the modules, and the plugin's decides the plugin's rooms.
    let outside = [
        support::app(support::options())
            .module(SupportDeskOutside)
            .module(ChatModule),
        support::app(support::options())
            .module(ChatModule)
            .module(SupportDeskOutside),
    ];
    for builder in outside {
        let server = Server::serve(builder).await;
        let gurpreet = Uuid::now_v7();
        let desk = server
            .auth(Some(gurpreet), SOCKET, "private-support.desk")
            .await;
        assert_eq!(desk.status, StatusCode::FORBIDDEN, "{}", desk.json);
        let room = server.auth(Some(gurpreet), SOCKET, "private-chat.7").await;
        assert_eq!(room.status, StatusCode::OK, "{}", room.json);
    }

    // The app registers no pattern of its own: `chat.support` is left to the plugin's
    // `chat.{room}`, which decides it as it decides every room.
    let server = Server::serve(support::app(support::options()).module(ChatModule)).await;
    let channel = "private-chat.support";
    let reply = server.auth(Some(Uuid::now_v7()), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    assert_eq!(reply.json["auth"], signature(SOCKET, channel, None));
}

#[tokio::test]
async fn the_apps_channels_under_words_no_plugin_holds_boot_beside_a_plugin() {
    // The test app's `crm.deals`, `users.{id}`, `room.{id}` and `broken`, and `orders.{id}`,
    // beside the chat plugin.
    let server = Server::serve(
        support::app(support::options())
            .module(ChatModule)
            .module(OrdersModule),
    )
    .await;
    let channel = "private-orders.7";
    let reply = server.auth(Some(Uuid::now_v7()), SOCKET, channel).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
    let harpreet = Uuid::now_v7();
    let own = format!("private-users.{harpreet}");
    let reply = server.auth(Some(harpreet), SOCKET, &own).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
}

/// A module of the app whose pattern `{team}.support` starts with a parameter: for a team
/// named `chat`, `private-chat.support` matches both `{team}.support` and the chat plugin's
/// `chat.{room}`.
struct TeamSupport(Uuid);

impl Module for TeamSupport {
    fn register(&self, m: &mut ModuleBuilder) {
        let lead = self.0;
        m.channel("{team}.support", move |principal, params, _ctx| {
            let allowed = principal.user_id == lead && params.get("team").is_some();
            Box::pin(async move {
                Ok(if allowed {
                    ChannelDecision::Allow
                } else {
                    ChannelDecision::Deny
                })
            })
        });
    }
}

/// A module of the app that imports the chat plugin's module first in dependency order and
/// registers `{team}.support` for its support lead.
struct TeamSupportWithChat(Uuid);

impl Module for TeamSupportWithChat {
    fn register(&self, m: &mut ModuleBuilder) {
        let lead = self.0;
        m.import(ChatModule)
            .channel("{team}.support", move |principal, params, _ctx| {
                let allowed = principal.user_id == lead && params.get("team").is_some();
                Box::pin(async move {
                    Ok(if allowed {
                        ChannelDecision::Allow
                    } else {
                        ChannelDecision::Deny
                    })
                })
            });
    }
}

#[tokio::test]
async fn an_app_pattern_that_starts_with_a_parameter_is_decided_before_a_plugins_pattern_in_either_module_order()
 {
    let simran = Uuid::now_v7();
    let gurpreet = Uuid::now_v7();
    let orders = [
        support::app(support::options())
            .module(TeamSupport(simran))
            .module(ChatModule),
        support::app(support::options())
            .module(ChatModule)
            .module(TeamSupport(simran)),
        support::app(support::options()).module(TeamSupportWithChat(simran)),
    ];
    for builder in orders {
        let server = Server::serve(builder).await;
        let denied = server
            .auth(Some(gurpreet), SOCKET, "private-chat.support")
            .await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.json);

        let allowed = server
            .auth(Some(simran), SOCKET, "private-chat.support")
            .await;
        assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.json);
        assert_eq!(
            allowed.json["auth"],
            signature(SOCKET, "private-chat.support", None)
        );
    }
}

#[tokio::test]
async fn a_plugins_pattern_still_decides_the_channels_under_its_namespace() {
    let simran = Uuid::now_v7();
    let gurpreet = Uuid::now_v7();
    let orders = [
        support::app(support::options())
            .module(TeamSupport(simran))
            .module(ChatModule),
        support::app(support::options())
            .module(ChatModule)
            .module(TeamSupport(simran)),
        support::app(support::options()).module(TeamSupportWithChat(simran)),
    ];
    for builder in orders {
        let server = Server::serve(builder).await;
        let channel = "private-chat.ludhiana";
        let reply = server.auth(Some(gurpreet), SOCKET, channel).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.json);
        assert_eq!(reply.json["auth"], signature(SOCKET, channel, None));
    }
}

/// A module of the app: it registers the chat plugin's backplane as the app's.
struct AppBackplane;

impl Module for AppBackplane {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(navree_plugin_chat::backplane());
    }
}

#[tokio::test]
async fn a_plugin_may_not_provide_the_apps_backplane() {
    // With `realtime.backplane = none` the app takes the backplane a module provides, so the
    // plugin's would carry every private and presence broadcast of the app.
    let refused = support::app(support::options())
        .module(ChatBackplane)
        .validate()
        .expect_err("the plugin provides a backplane")
        .to_string();
    let expected = format!(
        "plugin `navree-plugin-chat` provides `{}` in `ChatBackplane`, which carries every broadcast of the app's channels between its processes; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and to use the plugin's, register it from a module of the app",
        type_name::<dyn Backplane>()
    );
    assert!(refused.contains(&expected), "{refused}");

    // A module of the app registers the plugin's backplane beside the plugin, as before.
    let server = Server::serve(
        support::app(support::options())
            .module(ChatModule)
            .module(AppBackplane),
    )
    .await;
    let backplane = server.app.ctx().try_get::<dyn Backplane>().await;
    assert!(
        backplane.is_some(),
        "the app's module registered the plugin's backplane"
    );
}

mod notice_board_core {
    use navree_kernel::{Module, ModuleBuilder};
    use navree_realtime::ModuleBuilderRealtimeExt;

    pub struct SecondCrateChannel;

    impl Module for SecondCrateChannel {
        fn register(&self, m: &mut ModuleBuilder) {
            m.channel("users.{id}", super::navree_plugin_notice_board::anyone);
        }
    }
}

mod navree_plugin_notice_board_importer {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    const IMPORTER: PluginInfo = PluginInfo::new(
        "navree-plugin-board-importer",
        module_path!(),
        "board_importer",
        "1.0.0",
    );

    pub struct ImporterPlugin;

    impl Module for ImporterPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(IMPORTER)
                .import(super::notice_board_core::SecondCrateChannel);
        }
    }
}

#[tokio::test]
async fn the_refusal_says_why_the_module_acts_for_the_plugin() {
    // 1. Module of the plugin's crate (`Helper`).
    let in_crate = support::app(support::options())
        .module(BoardModule("notice_board.{id}"))
        .module(navree_plugin_notice_board::Helper("users.{id}"))
        .boot()
        .await
        .expect_err("refused")
        .to_string();
    let expected_crate = format!(
        "channel pattern `users.{{id}}` of module `Helper` (a module of the plugin's crate `{}`): plugin `navree-plugin-notice-board` authorizes channels under `notice_board.` only",
        navree_plugin_notice_board::NOTICE_BOARD.crate_name
    );
    assert!(in_crate.contains(&expected_crate), "{in_crate}");

    // 2. Module of a second crate imported only by the plugin (`SecondCrateChannel`).
    let by_import = support::app(support::options())
        .module(navree_plugin_notice_board_importer::ImporterPlugin)
        .boot()
        .await
        .expect_err("refused")
        .to_string();
    assert!(
        by_import.contains(
            "channel pattern `users.{id}` of module `SecondCrateChannel` (only the plugin's modules import it): plugin `navree-plugin-board-importer` authorizes channels under `board_importer.` only"
        ),
        "{by_import}"
    );
}
