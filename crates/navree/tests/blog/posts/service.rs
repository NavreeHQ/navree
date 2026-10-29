//! The business logic of the posts module.

use chrono::Utc;
use navree::mail::{Mailer, TemplateMail};
use navree::orm::access::AccessibleQuery as _;
use navree::orm::{Model as _, OrmError, Record};
use navree::prelude::*;
use serde_json::json;

use super::dto::{CreatePost, UpdatePost};
use super::entity::Post;

/// Lists, writes, edits and publishes posts. Writes go through the model's
/// records in the caller's unit of work, so they commit (and are audited)
/// with it.
#[injectable]
pub struct PostService {
    mailer: Arc<Mailer>,
}

impl PostService {
    /// One page of the posts `scope` lets the caller read, newest first.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Post, Read>,
        page: PageQuery,
    ) -> Result<Page<Record<Post>>, OrmError> {
        scope
            .query()
            .order_by_desc(Post::CREATED_AT)
            .order_by_desc(Post::ID)
            .paginate(uow, page)
            .await
    }

    /// Writes a draft by the acting user.
    pub async fn create(&self, uow: &Uow, input: CreatePost) -> Result<Record<Post>> {
        let author = uow.actor().ok_or_else(AppError::unauthenticated)?;
        Ok(Post::create(
            uow,
            Post {
                author_id: author,
                title: input.title,
                body: input.body,
                ..Post::default()
            },
        )
        .await?)
    }

    /// Applies the fields present in `input`.
    pub async fn update(
        &self,
        uow: &Uow,
        mut post: Record<Post>,
        input: UpdatePost,
    ) -> Result<Record<Post>, OrmError> {
        if let Some(title) = input.title {
            post.title = title;
        }
        if let Some(body) = input.body {
            post.body = body;
        }
        post.save(uow).await?;
        Ok(post)
    }

    /// Publishes `post` and queues a mail to its author (`author_email`,
    /// greeted as `author_name`). The mail job joins the unit of work: it is
    /// queued only if the publication commits.
    pub async fn publish(
        &self,
        uow: &Uow,
        mut post: Record<Post>,
        author_name: &str,
        author_email: &str,
    ) -> Result<Record<Post>> {
        post.published = true;
        post.published_at = Some(Utc::now());
        post.save(uow).await?;
        let mail = TemplateMail::new(author_email, "Your post is live", "mail/post_published")
            .context(json!({ "name": author_name, "title": post.title }));
        self.mailer.queue(&mail).await?;
        Ok(post)
    }
}
