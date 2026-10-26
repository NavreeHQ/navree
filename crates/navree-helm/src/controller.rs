//! [`HelmController`]: every route of the panel, under `helm.path`.

use std::sync::Arc;

use http::Method;
use navree_http::{Controller, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::core::Core;
use crate::{HELM_SUBJECT, VIEW, api, ui};

/// The panel's routes (the pages and the JSON API under `{helm.path}/api`).
/// Every route but the sign-in screen and the assets requires `view` on
/// `Helm`; each handler then checks the user's abilities on the resource
/// and the record.
pub struct HelmController {
    core: Arc<Core>,
}

impl Injectable for HelmController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Core>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                core: r.get::<Core>().await?,
            })
        })
    }
}

impl Controller for HelmController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let base = self.core.options.base().to_owned();
        let at = |path: &str| format!("!{base}{path}");
        let view = |route: RouteDef| route.can(VIEW, HELM_SUBJECT, None);
        vec![
            // Assets and sign-in.
            RouteDef::on(Method::GET, at("/assets/{file}"), ui::asset)
                .public()
                .name("helm.assets"),
            RouteDef::on(Method::GET, at("/login"), ui::login_form)
                .public()
                .name("helm.login"),
            RouteDef::on(Method::POST, at("/login"), ui::login)
                .public()
                .name("helm.login.attempt"),
            RouteDef::on(Method::POST, at("/login/two-factor"), ui::two_factor)
                .public()
                .name("helm.login.two_factor"),
            RouteDef::on(Method::POST, at("/logout"), ui::logout)
                .authenticated()
                .name("helm.logout"),
            // Pages.
            view(RouteDef::on(Method::GET, at(""), ui::dashboard).name("helm.dashboard")),
            view(
                RouteDef::on(Method::GET, at("/resources/{resource}"), ui::list).name("helm.list"),
            ),
            view(
                RouteDef::on(Method::POST, at("/resources/{resource}"), ui::create)
                    .name("helm.create"),
            ),
            view(
                RouteDef::on(Method::GET, at("/resources/{resource}/new"), ui::new_form)
                    .name("helm.new"),
            ),
            view(
                RouteDef::on(
                    Method::POST,
                    at("/resources/{resource}/actions"),
                    ui::run_action,
                )
                .name("helm.action"),
            ),
            view(
                RouteDef::on(Method::GET, at("/resources/{resource}/{id}"), ui::show)
                    .name("helm.show"),
            ),
            view(
                RouteDef::on(Method::POST, at("/resources/{resource}/{id}"), ui::update)
                    .name("helm.update"),
            ),
            view(
                RouteDef::on(
                    Method::GET,
                    at("/resources/{resource}/{id}/edit"),
                    ui::edit_form,
                )
                .name("helm.edit"),
            ),
            view(
                RouteDef::on(
                    Method::POST,
                    at("/resources/{resource}/{id}/delete"),
                    ui::destroy,
                )
                .name("helm.delete"),
            ),
            view(RouteDef::on(Method::GET, at("/pages/{page}"), ui::custom_page).name("helm.page")),
            // The JSON API.
            view(
                RouteDef::on(Method::GET, at("/api/resources"), api::index)
                    .name("helm.api.resources"),
            ),
            view(
                RouteDef::on(Method::GET, at("/api/resources/{resource}"), api::list)
                    .name("helm.api.list"),
            ),
            view(
                RouteDef::on(Method::POST, at("/api/resources/{resource}"), api::create)
                    .name("helm.api.create"),
            ),
            view(
                RouteDef::on(
                    Method::GET,
                    at("/api/resources/{resource}/options"),
                    api::options,
                )
                .name("helm.api.options"),
            ),
            view(
                RouteDef::on(Method::GET, at("/api/resources/{resource}/{id}"), api::show)
                    .name("helm.api.show"),
            ),
            view(
                RouteDef::on(
                    Method::PATCH,
                    at("/api/resources/{resource}/{id}"),
                    api::update,
                )
                .name("helm.api.update"),
            ),
            view(
                RouteDef::on(
                    Method::DELETE,
                    at("/api/resources/{resource}/{id}"),
                    api::delete,
                )
                .name("helm.api.delete"),
            ),
            view(
                RouteDef::on(
                    Method::POST,
                    at("/api/resources/{resource}/actions/{action}"),
                    api::action,
                )
                .name("helm.api.action"),
            ),
            view(
                RouteDef::on(
                    Method::GET,
                    at("/api/resources/{resource}/metrics/{metric}"),
                    api::metric,
                )
                .name("helm.api.metric"),
            ),
            view(
                RouteDef::on(Method::POST, at("/api/markdown"), api::markdown_preview)
                    .name("helm.api.markdown"),
            ),
        ]
    }
}
