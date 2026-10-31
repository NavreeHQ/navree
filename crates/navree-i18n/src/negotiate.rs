//! Locale negotiation: the sources of `i18n.negotiation`, tried in order
//! for each request, and the layer that makes the result current.

use std::sync::{Arc, OnceLock};

use axum::extract::{Request, State};
use axum::middleware::Next;
use http::header::{ACCEPT_LANGUAGE, CONTENT_LANGUAGE, COOKIE, HeaderValue};
use http::request::Parts;
use navree_contracts::AuthState;
use navree_http::response::Response;
use navree_kernel::AppContext;
use navree_workspace::{CurrentWorkspace, WorkspaceConfig, Workspaces, requested_workspace};
use uuid::Uuid;

use crate::I18n;
use crate::locale::Locale;
use crate::options::Source;
use crate::service::{Owner, Shared};

/// At most this many `Accept-Language` entries are considered.
const MAX_ACCEPT_LANGUAGES: usize = 16;

/// The tags of an `Accept-Language` header, by quality (highest first,
/// header order among equals); `*` and `q=0` entries are dropped.
pub(crate) fn accept_language(header: &str) -> Vec<String> {
    let mut entries: Vec<(String, u16)> = header
        .split(',')
        .take(MAX_ACCEPT_LANGUAGES)
        .filter_map(|entry| {
            let mut parts = entry.split(';');
            let tag = parts.next()?.trim();
            if tag.is_empty() || tag == "*" {
                return None;
            }
            let mut quality = 1000;
            for parameter in parts {
                let parameter = parameter.trim();
                if let Some(value) = parameter
                    .strip_prefix("q=")
                    .or_else(|| parameter.strip_prefix("Q="))
                {
                    let value: f32 = value.trim().parse().ok()?;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        quality = (value.clamp(0.0, 1.0) * 1000.0).round() as u16;
                    }
                }
            }
            (quality > 0).then(|| (tag.to_owned(), quality))
        })
        .collect();
    // A stable sort keeps header order among equal qualities.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    entries.into_iter().map(|(tag, _)| tag).collect()
}

/// The value of cookie `name` in a `Cookie` header.
pub(crate) fn cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// The value of query parameter `name`.
pub(crate) fn query_param(query: &str, name: &str) -> Option<String> {
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// The locale of a request: the first source of `i18n.negotiation` that
/// names a supported locale, else the default one.
pub(crate) async fn negotiate(shared: &Arc<Shared>, parts: &Parts) -> Locale {
    for source in &shared.options.negotiation {
        if let Some(locale) = from_source(shared, *source, parts).await {
            return locale;
        }
    }
    shared.default_locale()
}

async fn from_source(shared: &Arc<Shared>, source: Source, parts: &Parts) -> Option<Locale> {
    match source {
        Source::Query => {
            let value = query_param(parts.uri.query()?, &shared.options.query_param)?;
            shared.matching(&value)
        }
        Source::Cookie => parts
            .headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|header| header.to_str().ok())
            .find_map(|header| cookie(header, &shared.options.cookie))
            .and_then(|value| shared.matching(&value)),
        Source::Header => {
            let header = parts.headers.get(ACCEPT_LANGUAGE)?.to_str().ok()?;
            accept_language(header)
                .iter()
                .find_map(|tag| shared.matching(tag))
        }
        Source::User => {
            let user = user_id(parts)?;
            saved(shared, Owner::User, user).await
        }
        Source::Workspace => {
            let workspace = request_workspace(parts).await?;
            saved(shared, Owner::Workspace, workspace).await
        }
    }
}

/// The signed-in user's id.
fn user_id(parts: &Parts) -> Option<Uuid> {
    parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id)
}

/// A saved locale; a database error is logged and skipped.
async fn saved(shared: &Arc<Shared>, owner: Owner, id: Uuid) -> Option<Locale> {
    match shared.saved(owner, id).await {
        Ok(locale) => locale,
        Err(error) => {
            tracing::warn!(
                target: "navree::i18n",
                %error,
                "could not read a saved locale; trying the next source"
            );
            None
        }
    }
}

/// The id of the workspace the request names, when the app has workspaces
/// and the signed-in user is a member of it. Nothing is entered or cached:
/// the request's own `CurrentWorkspace` does that later.
async fn request_workspace(parts: &Parts) -> Option<Uuid> {
    if let Some(current) = parts.extensions.get::<CurrentWorkspace>() {
        return Some(current.id());
    }
    let user = user_id(parts)?;
    let ctx = parts.extensions.get::<AppContext>()?;
    let config = ctx.try_get::<WorkspaceConfig>().await?;
    let (_, identifier) = requested_workspace(&config, parts)?;
    let workspaces = ctx.try_get::<Workspaces>().await?;
    let workspace = workspaces.resolve(&identifier).await.ok()?;
    workspaces
        .membership(workspace.id, user)
        .await
        .ok()
        .flatten()
        .map(|_| workspace.id)
}

/// The middleware: negotiates the request's locale, puts it in the request
/// extensions (the [`Locale`] extractor), runs the request with it current,
/// and adds `Content-Language` to the response.
pub(crate) async fn layer(
    State(slot): State<Arc<OnceLock<Arc<I18n>>>>,
    request: Request,
    next: Next,
) -> Response {
    let ctx = request.extensions().get::<AppContext>().cloned();
    let Some(i18n) = i18n_of(&slot, ctx).await else {
        return next.run(request).await;
    };
    let (mut parts, body) = request.into_parts();
    let locale = negotiate(&i18n.shared, &parts).await;
    parts.extensions.insert(locale.clone());
    let request = Request::from_parts(parts, body);
    let tag = locale.tag().to_owned();
    let mut response = Locale::scope(locale, next.run(request)).await;
    if let Ok(value) = HeaderValue::from_str(&tag) {
        response
            .headers_mut()
            .entry(CONTENT_LANGUAGE)
            .or_insert(value);
    }
    response
}

/// The app's `I18n`, resolved on the first request.
async fn i18n_of(slot: &OnceLock<Arc<I18n>>, ctx: Option<AppContext>) -> Option<Arc<I18n>> {
    if let Some(i18n) = slot.get() {
        return Some(i18n.clone());
    }
    let i18n = ctx?.try_get::<I18n>().await?;
    Some(slot.get_or_init(|| i18n).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_orders_by_quality() {
        assert_eq!(
            accept_language("fr-CA;q=0.9, en;q=0.8, de, *;q=0.1"),
            ["de", "fr-CA", "en"]
        );
        assert_eq!(accept_language("fr-FR,fr;q=0.9"), ["fr-FR", "fr"]);
        assert_eq!(accept_language("en;q=0, fr"), ["fr"]);
        assert_eq!(accept_language("en;q=bad, fr"), ["fr"]);
        assert!(accept_language("").is_empty());
    }

    #[test]
    fn cookies_and_query_parameters() {
        assert_eq!(
            cookie("a=1; locale=fr; b=2", "locale"),
            Some("fr".to_owned())
        );
        assert_eq!(cookie("locale=\"de\"", "locale"), Some("de".to_owned()));
        assert_eq!(cookie("a=1", "locale"), None);
        assert_eq!(
            query_param("page=2&locale=pt-BR", "locale"),
            Some("pt-BR".to_owned())
        );
        assert_eq!(query_param("page=2", "locale"), None);
    }
}
