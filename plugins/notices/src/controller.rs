//! HTTP page and JSON endpoints for the notice board.

use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use http::Method;
use navree::contracts::Clock;
use navree::db::Uow;
use navree::http::{
    AppError, Controller, Created, CurrentPrincipal, Json, NoContent, Path, Result, RouteDef,
};
use navree::kernel::{AppContext, BoxFuture, Injectable, KernelError, Resolver};
use navree::orm::{Model as _, Record};
use navree::ui::{AppPage, AppShell};
use navree::validation::{Valid, Validate};
use navree::view::Views;
use serde::{Deserialize, Serialize};

use crate::model::notice;
use crate::{options, pages};

/// Validated payload for `POST /api/notices`.
#[derive(Clone, Debug, Deserialize, Validate)]
pub struct NewNotice {
    /// Short heading of the notice (1 to 120 characters).
    #[validate(length(min = 1, max = 120))]
    pub title: String,
    /// Full body text (at most 4,000 characters).
    #[serde(default)]
    #[validate(length(max = 4000))]
    pub body: String,
}

/// Template variables passed to `notices/list.html`.
#[derive(Serialize)]
struct ListView {
    notices: Vec<notice::Model>,
}

/// Serves `GET /notices` and the JSON routes under `/api/notices`.
pub struct NoticesController {
    app: AppContext,
}

impl Injectable for NoticesController {
    fn construct(r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        let app = r.app();
        Box::pin(async move { Ok(Self { app }) })
    }
}

impl Controller for NoticesController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let page_ctrl = self.clone();
        let list_ctrl = self.clone();
        let create_ctrl = self.clone();
        let remove_ctrl = self;
        vec![
            RouteDef::on(
                Method::GET,
                "!/notices",
                move |shell: AppShell, uow: Uow| {
                    let ctrl = page_ctrl.clone();
                    async move { ctrl.board(shell, uow).await }
                },
            )
            .page()
            .permissions(&["notices.read"]),
            RouteDef::on(Method::GET, "/notices", move |uow: Uow| {
                let ctrl = list_ctrl.clone();
                async move { ctrl.list_json(uow).await }
            })
            .permissions(&["notices.read"]),
            RouteDef::on(
                Method::POST,
                "/notices",
                move |principal: CurrentPrincipal, uow: Uow, input: Valid<Json<NewNotice>>| {
                    let ctrl = create_ctrl.clone();
                    async move { ctrl.create(principal, uow, input).await }
                },
            )
            .permissions(&["notices.publish"]),
            RouteDef::on(
                Method::DELETE,
                "/notices/{id}",
                move |id: Path<i64>, uow: Uow| {
                    let ctrl = remove_ctrl.clone();
                    async move { ctrl.remove(id, uow).await }
                },
            )
            .permissions(&["notices.publish"]),
        ]
    }
}

impl NoticesController {
    async fn active_options(&self) -> Result<options::NoticesOptions> {
        let settings = options::load(&self.app).await.map_err(AppError::internal)?;
        if !settings.enabled {
            return Err(AppError::not_found());
        }
        Ok(settings)
    }

    async fn board(&self, shell: AppShell, uow: Uow) -> Result<AppPage> {
        self.active_options().await?;
        let notices = notice::Model::query()
            .latest()
            .get(&uow)
            .await
            .map_err(AppError::internal)?
            .into_models();
        let views = self.app.get::<Views>().await.map_err(AppError::internal)?;
        let html = views
            .render("notices/list.html", ListView { notices })
            .map_err(AppError::internal)?;
        Ok(pages::board_page(shell, "Notices", html))
    }

    async fn list_json(&self, uow: Uow) -> Result<Json<Vec<notice::Model>>> {
        self.active_options().await?;
        let rows = notice::Model::query()
            .latest()
            .get(&uow)
            .await
            .map_err(AppError::internal)?
            .into_models();
        Ok(Json(rows))
    }

    async fn create(
        &self,
        CurrentPrincipal(principal): CurrentPrincipal,
        uow: Uow,
        Valid(Json(input)): Valid<Json<NewNotice>>,
    ) -> Result<Created<notice::Model>> {
        let settings = self.active_options().await?;
        let now = match self.app.try_get::<dyn Clock>().await {
            Some(clock) => clock.now(),
            None => Utc::now(),
        };
        let mut record = Record::new(notice::Model {
            id: 0,
            title: input.title,
            body: input.body,
            posted_by: principal.user_id,
            expires_at: now + TimeDelta::days(i64::from(settings.keep_days)),
            created_at: now,
        });
        record.save(&uow).await.map_err(AppError::internal)?;
        uow.commit().await.map_err(AppError::internal)?;
        let row = record.into_inner();
        Ok(Created(format!("/api/notices/{}", row.id), row))
    }

    async fn remove(&self, Path(id): Path<i64>, uow: Uow) -> Result<NoContent> {
        self.active_options().await?;
        let deleted = notice::Model::query()
            .where_key(id)
            .delete(&uow)
            .await
            .map_err(AppError::internal)?;
        if deleted == 0 {
            return Err(AppError::not_found());
        }
        uow.commit().await.map_err(AppError::internal)?;
        Ok(NoContent)
    }
}
