//! [`HelmPage`]: a screen that is not a resource, rendered from one of your
//! templates inside the panel's layout.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_http::AppError;
use navree_kernel::{BoxFuture, ModuleId};
use serde_json::Value;

use crate::PageContext;

/// The data function of a page.
pub(crate) type DataFn =
    Arc<dyn Fn(PageContext) -> BoxFuture<'static, Result<Value, AppError>> + Send + Sync>;

/// A custom page of the panel, served at `{helm.path}/pages/{slug}` and
/// listed in the menu for users who pass its check. Register it with
/// `m.helm_page(..)` ([`ModuleBuilderHelmExt::helm_page`](crate::ModuleBuilderHelmExt::helm_page)).
///
/// ```ignore
/// m.helm_page(
///     HelmPage::new("pipeline", "Pipeline board")
///         .group("Sales")
///         .icon("kanban")
///         .template("admin/pipeline")
///         .can("read", "Deal"),
/// );
/// ```
///
/// The template (a navree-view template; the app needs `ViewModule`) gets
/// `title`, `user` (`{id, name, email}`), `csrf_token` (the session's token,
/// masked afresh for each page, for a form's `_csrf` field), `path` (the panel's
/// path) and `data` (what [`data`](HelmPage::data) returns, else `null`).
#[derive(Clone)]
pub struct HelmPage {
    pub(crate) slug: String,
    pub(crate) title: String,
    pub(crate) group: Option<String>,
    pub(crate) icon: String,
    pub(crate) template: Option<String>,
    pub(crate) can: Option<(String, String)>,
    pub(crate) data: Option<DataFn>,
}

impl HelmPage {
    /// A page at `pages/{slug}` titled `title`.
    pub fn new(slug: &str, title: &str) -> Self {
        Self {
            slug: crate::slug(slug),
            title: title.to_owned(),
            group: None,
            icon: String::new(),
            template: None,
            can: None,
            data: None,
        }
    }

    /// The menu group (default: **Pages**).
    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_owned());
        self
    }

    /// A Lucide icon name (in the metadata API).
    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = icon.to_owned();
        self
    }

    /// The template rendered inside the panel's layout.
    pub fn template(mut self, name: &str) -> Self {
        self.template = Some(name.to_owned());
        self
    }

    /// Shows the page only to users who may do `action` on `subject`
    /// (a type-level check); everyone with `view` on `Helm` otherwise.
    pub fn can(mut self, action: &str, subject: &str) -> Self {
        self.can = Some((action.to_owned(), subject.to_owned()));
        self
    }

    /// Provides the template's `data`: `f(ctx)` returns a JSON value.
    pub fn data<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(PageContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, AppError>> + Send + 'static,
    {
        self.data = Some(Arc::new(move |ctx| Box::pin(f(ctx))));
        self
    }

    /// The page's slug.
    pub fn slug(&self) -> &str {
        &self.slug
    }
}

impl fmt::Debug for HelmPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HelmPage")
            .field("slug", &self.slug)
            .field("title", &self.title)
            .field("template", &self.template)
            .field("can", &self.can)
            .finish_non_exhaustive()
    }
}

/// A page registered by a module (a contribution).
#[derive(Clone, Debug)]
pub(crate) struct PageRegistration {
    pub(crate) page: HelmPage,
    pub(crate) module: ModuleId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_slug_their_names() {
        let page = HelmPage::new("Pipeline Board", "Pipeline board").can("read", "Deal");
        assert_eq!(page.slug(), "pipeline-board");
        assert_eq!(page.can, Some(("read".into(), "Deal".into())));
    }
}
