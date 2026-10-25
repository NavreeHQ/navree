//! The posts module: the `posts` table, drafts, publishing (with a queued
//! mail to the author) and the JSON API under `/api/v1/posts`.

mod controller;
pub mod dto;
pub mod entity;
mod migrations;
mod service;

use keel::prelude::*;

pub use controller::PostController;
pub use service::PostService;

/// The `publish` action: `#[can(Publish, Post, from = "id")]`,
/// `Authorized<Post, Publish>`.
pub struct Publish;

impl ActionMarker for Publish {
    const NAME: &'static str = "publish";
}

/// Posts: CRUD-ish routes, each checking the caller's abilities on `Post`.
#[module(
    imports = [AccessModule::for_feature::<entity::Entity>()],
    providers = [PostService],
    controllers = [PostController],
    exports = [PostService],
    register = Self::wire,
)]
pub struct PostsModule;

impl PostsModule {
    /// What `#[module]` has no key for: migrations and mail templates.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        m.templates(&[
            (
                "mail/post_published.txt",
                "Hi {{ name }}, \"{{ title }}\" is live.",
            ),
            (
                "mail/post_published.html",
                "<p>Hi {{ name }}, <strong>{{ title }}</strong> is live.</p>",
            ),
        ]);
    }
}
