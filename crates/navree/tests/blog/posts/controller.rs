//! The posts JSON API: `/api/v1/posts`.

use navree::orm::Record;
use navree::prelude::*;

use super::Publish;
use super::dto::{CreatePost, UpdatePost};
use super::entity::Post;
use super::service::PostService;

/// The routes of the posts resource. `from = "id"` routes load the post
/// first (404 when it does not exist, 403 when the rules say no).
#[injectable]
pub struct PostController {
    posts: Arc<PostService>,
}

#[controller("/posts", version = "1")]
impl PostController {
    /// Guests see published posts; signed-in users also their own drafts.
    #[get("/")]
    #[public]
    #[api(summary = "List the posts the caller may read", tag = "Posts")]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Post, Read>,
        page: PageQuery,
    ) -> Result<Page<Record<Post>>> {
        Ok(self.posts.list(&uow, scope, page).await?)
    }

    /// One post.
    #[get("/{id}")]
    #[can(Read, Post, from = "id")]
    #[api(summary = "Show a post", tag = "Posts", responses(403, 404))]
    async fn show(&self, post: Authorized<Post, Read>) -> Json<Record<Post>> {
        Json(post.into_inner())
    }

    /// Writes a draft.
    #[post("/")]
    #[can(Create, Post)]
    #[api(
        summary = "Write a draft",
        tag = "Posts",
        request = CreatePost,
        status = 201,
        responses(400)
    )]
    async fn store(
        &self,
        uow: Uow,
        Valid(Json(input)): Valid<Json<CreatePost>>,
    ) -> Result<Created<Record<Post>>> {
        let post = self.posts.create(&uow, input).await?;
        Ok(Created(format!("/api/v1/posts/{}", post.id), post))
    }

    /// Edits the author's own post.
    #[patch("/{id}")]
    #[can(Update, Post, from = "id")]
    #[api(
        summary = "Edit a post",
        tag = "Posts",
        request = UpdatePost,
        responses(400, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        post: Authorized<Post, Update>,
        Valid(Json(input)): Valid<Json<UpdatePost>>,
    ) -> Result<Json<Record<Post>>> {
        post.check_fields(&input.changed_fields())?;
        Ok(Json(
            self.posts.update(&uow, post.into_inner(), input).await?,
        ))
    }

    /// Publishes the author's own post and mails them.
    #[post("/{id}/publish")]
    #[can(Publish, Post, from = "id")]
    #[api(summary = "Publish a post", tag = "Posts", responses(403, 404))]
    async fn publish(
        &self,
        uow: Uow,
        CurrentUser(author): CurrentUser,
        post: Authorized<Post, Publish>,
    ) -> Result<Json<Record<Post>>> {
        let post = self
            .posts
            .publish(&uow, post.into_inner(), &author.name, &author.email)
            .await?;
        Ok(Json(post))
    }
}
