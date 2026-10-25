//! The business logic of the posts module.

use chrono::Utc;
use keel::db::paginate;
use keel::mail::{Mailer, TemplateMail};
use keel::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{DbErr, EntityTrait, QueryOrder};
use serde_json::json;
use uuid::Uuid;

use super::dto::{CreatePost, UpdatePost};
use super::entity::{ActiveModel, Column, Entity, Model};

/// Lists, writes, edits and publishes posts. Writes go through `Repo` in
/// the caller's unit of work, so they commit (and are audited) with it.
#[injectable]
pub struct PostService {
    mailer: Arc<Mailer>,
}

impl PostService {
    /// One page of the posts `scope` lets the caller read, newest first.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Entity, Read>,
        page: PageQuery,
    ) -> Result<Page<Model>, DbErr> {
        let select = scope.apply(
            Entity::find()
                .order_by_desc(Column::CreatedAt)
                .order_by_desc(Column::Id),
        );
        paginate(select, &*uow.tx().await?, page).await
    }

    /// Writes a draft by the acting user.
    pub async fn create(&self, uow: &Uow, input: CreatePost) -> Result<Model> {
        let author = uow.actor().ok_or_else(AppError::unauthenticated)?;
        let now = Utc::now();
        let post = ActiveModel {
            id: Set(Uuid::now_v7()),
            author_id: Set(author),
            title: Set(input.title),
            body: Set(input.body),
            published: Set(false),
            published_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(Repo::<Entity>::insert(uow, post).await?)
    }

    /// Applies the fields present in `input`.
    pub async fn update(&self, uow: &Uow, post: Model, input: UpdatePost) -> Result<Model, DbErr> {
        let mut active: ActiveModel = post.into();
        if let Some(title) = input.title {
            active.title = Set(title);
        }
        if let Some(body) = input.body {
            active.body = Set(body);
        }
        active.updated_at = Set(Utc::now());
        Repo::<Entity>::update(uow, active).await
    }

    /// Publishes `post` and queues a mail to its author (`author_email`,
    /// greeted as `author_name`). The mail job joins the unit of work: it is
    /// queued only if the publication commits.
    pub async fn publish(
        &self,
        uow: &Uow,
        post: Model,
        author_name: &str,
        author_email: &str,
    ) -> Result<Model> {
        let now = Utc::now();
        let mut active: ActiveModel = post.into();
        active.published = Set(true);
        active.published_at = Set(Some(now));
        active.updated_at = Set(now);
        let post = Repo::<Entity>::update(uow, active).await?;
        let mail = TemplateMail::new(author_email, "Your post is live", "mail/post_published")
            .context(json!({ "name": author_name, "title": post.title }));
        self.mailer.queue(&mail).await?;
        Ok(post)
    }
}
