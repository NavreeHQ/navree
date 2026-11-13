//! A Navree UI notification list (the `ui` feature): [`NotificationBell`], the signed-in user's
//! stored notifications (the database channel) in the app's shell.
//!
//! The bell is a region of the page: the count of unread notifications and the latest few, each
//! a link to what it is about, with "Mark read" and "Mark all read". It fetches its list after
//! the page paints (so the shell stays fast), hears new notifications through the live hub when
//! the app has `RealtimeModule`, and polls once a minute otherwise.
//!
//! ```ignore
//! Shell::new("Agra").search(NotificationBell::new())
//! ```
//!
//! `NotifyModule` mounts the routes it uses when `notify.routes` is on and the app has a
//! database and an authenticator: `GET /notifications/bell` (the list), `POST
//! /notifications/{id}/mark-read` and `POST /notifications/read-all` (the runtime sends them with
//! the session's token; the buttons need the script). A notification's `data` is read as the
//! docs' `{title, message, url}`; `url` becomes the link only when it is an address on this site.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_contracts::BoxFuture;
use navree_db::{PageQuery, Uow};
use navree_http::{ApiMeta, AppError, Authenticator, Controller, CurrentPrincipal, Path, RouteDef};
use navree_kernel::{Injectable, KernelError, ProviderKey, Resolver};
use navree_ui::html::{Href, LocalHref};
use navree_ui::{Badge, Button, LiveStream, Node, Region, Reply, Size, View, el};
use uuid::Uuid;

use crate::controller::api;
use crate::{
    DatabaseNotification, INBOX_TOPIC, Notifiable, Notifier, NotifyOptions, USER_NOTIFIABLE_TYPE,
};

/// What the shell document says of push for a native client: the route a device's token is
/// posted to and deleted from, under the API prefix.
pub(crate) struct PushShellPart {
    path: String,
}

impl PushShellPart {
    /// The part for the routes under `api_prefix`.
    pub(crate) fn new(api_prefix: &str) -> Self {
        Self {
            path: format!("/{}/notifications/push-devices", api_prefix.trim_matches('/'))
                .replace("//", "/"),
        }
    }
}

impl navree_ui::NativeShellPart for PushShellPart {
    fn contribute(&self, shell: &mut navree_ui::NativeShellParts, _: &navree_ui::NativeClient) {
        shell.push_register(self.path.as_str());
    }
}

/// Where the list is fetched from.
pub const BELL_PATH: &str = "/notifications/bell";

/// The id of the bell's region, which the actions morph.
pub const BELL_ID: &str = "notification-bell";

/// How many notifications the list shows.
const SHOWN: u64 = 8;

/// How often the bell asks again when it hears nothing.
const POLL: Duration = Duration::from_secs(60);

/// The words of the bell. English by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BellLabels {
    /// The heading, and the name of the region.
    pub title: String,
    /// The list is empty.
    pub empty: String,
    /// Marks one notification read.
    pub mark_read: String,
    /// Marks every notification read.
    pub mark_all: String,
    /// The count of unread notifications, for a screen reader: `{n}`.
    pub unread: String,
}

impl Default for BellLabels {
    fn default() -> Self {
        Self {
            title: "Notifications".to_owned(),
            empty: "You're all caught up".to_owned(),
            mark_read: "Mark read".to_owned(),
            mark_all: "Mark all read".to_owned(),
            unread: "{n} unread".to_owned(),
        }
    }
}

/// What the bell draws: how many are unread, and the latest notifications.
#[derive(Clone, Debug)]
struct Inbox {
    unread: u64,
    items: Vec<DatabaseNotification>,
}

/// The signed-in user's notifications in the shell. See the [module](self).
///
/// ```
/// use navree_notify::ui::NotificationBell;
/// use navree_ui::render;
///
/// let html = render(&NotificationBell::new().into());
/// assert!(html.contains(r#"id="notification-bell""#));
/// assert!(html.contains(r#"data-ui-src="/notifications/bell""#));
/// assert!(html.contains("data-ui-lazy"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct NotificationBell {
    src: Href,
    poll: Duration,
    stream: Option<LiveStream>,
    labels: BellLabels,
    inbox: Option<Inbox>,
}

impl NotificationBell {
    /// A bell that fetches its list from `GET /notifications/bell`.
    pub fn new() -> Self {
        Self {
            src: Href::from(BELL_PATH),
            poll: POLL,
            stream: None,
            labels: BellLabels::default(),
            inbox: None,
        }
    }

    /// Fetches the list from `src` instead (a route of your own).
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        self.src = src.into();
        self
    }

    /// Asks again this often (a minute by default).
    pub fn poll(mut self, every: Duration) -> Self {
        self.poll = every;
        self
    }

    /// Hears a new notification on the app's live hub, so the list updates as one arrives.
    /// It needs `RealtimeModule`; without it the bell only polls.
    pub fn live(self) -> Self {
        self.live_on(LiveStream::topics([INBOX_TOPIC]))
    }

    /// [`live`](Self::live) on a stream of your own, which must carry the topic
    /// [`INBOX_TOPIC`].
    pub fn live_on(mut self, stream: LiveStream) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: BellLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Draws the list now, the way the fragment answers: `unread` of them are unread, and
    /// `items` are the latest, newest first.
    pub fn inbox(mut self, unread: u64, items: Vec<DatabaseNotification>) -> Self {
        self.inbox = Some(Inbox { unread, items });
        self
    }

    fn body(&self) -> Option<Node> {
        let inbox = self.inbox.as_ref()?;
        let words = &self.labels;
        let head = el("div").slot("notification-head").children((
            el("h2")
                .slot("notification-title")
                .child(words.title.clone()),
            (inbox.unread > 0).then(|| {
                el("span")
                    .slot("notification-count")
                    .attr(
                        "aria-label",
                        words.unread.replace("{n}", &inbox.unread.to_string()),
                    )
                    .child(Badge::new(inbox.unread.to_string()))
            }),
            (inbox.unread > 0).then(|| {
                el("form")
                    .attr("method", "post")
                    .attr("action", "/notifications/read-all")
                    .flag("data-ui-form", true)
                    .child(
                        Button::ghost(words.mark_all.clone())
                            .size(Size::Sm)
                            .submit(),
                    )
            }),
        ));
        let list: Node = if inbox.items.is_empty() {
            el("p")
                .slot("notification-empty")
                .child(words.empty.clone())
                .into()
        } else {
            el("ul")
                .slot("notification-list")
                .children(
                    inbox
                        .items
                        .iter()
                        .map(|item| self.row(item))
                        .collect::<Vec<_>>(),
                )
                .into()
        };
        Some(
            el("div")
                .slot("notification-bell")
                .attr("data-unread", inbox.unread.to_string())
                .attr("aria-label", words.title.clone())
                .children((head, list))
                .into(),
        )
    }

    fn row(&self, item: &DatabaseNotification) -> Node {
        let text = |key: &str| {
            item.data
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
        };
        let title = text("title").unwrap_or(&item.notification_type).to_owned();
        let heading: Node = match text("url").and_then(|url| LocalHref::parse(url).ok()) {
            Some(local) => el("a")
                .slot("notification-link")
                .attr("href", Href::from(local.as_str()))
                .child(title)
                .into(),
            None => el("span").slot("notification-link").child(title).into(),
        };
        let unread = item.read_at.is_none();
        el("li")
            .slot("notification")
            .flag("data-unread", unread)
            .children((
                heading,
                text("message").map(|message| {
                    el("p")
                        .slot("notification-message")
                        .child(message.to_owned())
                }),
                el("time")
                    .attr("datetime", item.created_at.to_rfc3339())
                    .child(item.created_at.format("%-d %b %Y, %H:%M UTC").to_string()),
                unread.then(|| {
                    el("form")
                        .attr("method", "post")
                        .attr("action", format!("/notifications/{}/mark-read", item.id))
                        .flag("data-ui-form", true)
                        .child(
                            Button::ghost(self.labels.mark_read.clone())
                                .size(Size::Sm)
                                .submit(),
                        )
                }),
            ))
            .into()
    }
}

impl Default for NotificationBell {
    fn default() -> Self {
        Self::new()
    }
}

impl From<NotificationBell> for Node {
    fn from(bell: NotificationBell) -> Self {
        let mut region = Region::new(BELL_ID).src(bell.src.clone()).poll(bell.poll);
        if let Some(stream) = &bell.stream {
            region = region.live_on(stream.on(INBOX_TOPIC));
        }
        match bell.body() {
            Some(body) => region.children(body).into(),
            // Fetched after the page paints.
            None => region.lazy().into(),
        }
    }
}

/// The signed-in user as a notifiable.
struct Reader(Uuid);

impl Notifiable for Reader {
    fn notifiable_type(&self) -> &'static str {
        USER_NOTIFIABLE_TYPE
    }

    fn notifiable_id(&self) -> String {
        self.0.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }
}

/// The routes of the bell, mounted by `NotifyModule` with its `ui` feature.
pub struct NotificationBellController {
    notifier: Arc<Notifier>,
    mounted: bool,
}

impl Injectable for NotificationBellController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Notifier>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let notifier = r.get::<Notifier>().await?;
            let app = r.app();
            let options: &NotifyOptions = notifier.options();
            let mounted = options.routes
                && app.try_get::<dyn Authenticator>().await.is_some()
                && app.try_get::<navree_db::Db>().await.is_some();
            Ok(Self { notifier, mounted })
        })
    }
}

/// The bell as it is now, for `me`.
async fn bell_of(
    notifier: &Notifier,
    uow: &Uow,
    me: &Reader,
) -> Result<NotificationBell, AppError> {
    let page = notifier
        .notifications(uow.db(), me, false, PageQuery::new(1, SHOWN))
        .await?;
    let unread = notifier.unread_count(uow.db(), me).await?;
    Ok(NotificationBell::new().live().inbox(unread, page.data))
}

impl Controller for NotificationBellController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.mounted {
            return Vec::new();
        }
        let list = self.clone();
        let read = self.clone();
        let read_all = self;
        vec![
            RouteDef::on(
                Method::GET,
                format!("!{BELL_PATH}"),
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow| {
                    let notifier = list.notifier.clone();
                    async move {
                        let bell = bell_of(&notifier, &uow, &Reader(me.user_id)).await?;
                        Ok::<_, AppError>(View::new("Notifications").body(bell))
                    }
                },
            )
            .authenticated()
            .name("notifications.bell")
            .api(ApiMeta {
                description: Some(
                    "The `NotificationBell` region as HTML: the unread count and the latest \
                     notifications, each a link to what it is about. A page asks for it after it \
                     paints, then again on a new notification (or once a minute without the live \
                     hub)."
                        .to_owned(),
                ),
                ..api("The signed-in user's notification bell", &[], None)
            }),
            RouteDef::on(
                Method::POST,
                "!/notifications/{id}/mark-read",
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow, Path(id): Path<String>| {
                    let notifier = read.notifier.clone();
                    async move {
                        let me = Reader(me.user_id);
                        let id = Uuid::parse_str(&id).map_err(|_| AppError::not_found())?;
                        if !notifier.mark_as_read(&uow, &me, id).await? {
                            return Err(AppError::not_found());
                        }
                        let bell = bell_of(&notifier, &uow, &me).await?;
                        Ok::<_, AppError>(Reply::ops_or_redirect("/").morph(BELL_ID, bell))
                    }
                },
            )
            .authenticated()
            .name("notifications.bell.read")
            .api(ApiMeta {
                description: Some(
                    "Answers the bell drawn again for the page's script, or a redirect (303) to \
                     `/` without it. Another user's notification is not found."
                        .to_owned(),
                ),
                ..api("Marks a notification read", &[303, 404], None)
            }),
            RouteDef::on(
                Method::POST,
                "!/notifications/read-all",
                move |CurrentPrincipal(me): CurrentPrincipal, uow: Uow| {
                    let notifier = read_all.notifier.clone();
                    async move {
                        let me = Reader(me.user_id);
                        notifier.mark_all_as_read(&uow, &me).await?;
                        let bell = bell_of(&notifier, &uow, &me).await?;
                        Ok::<_, AppError>(Reply::ops_or_redirect("/").morph(BELL_ID, bell))
                    }
                },
            )
            .authenticated()
            .name("notifications.bell.read_all")
            .api(ApiMeta {
                description: Some(
                    "Answers the bell drawn again for the page's script, or a redirect (303) to \
                     `/` without it."
                        .to_owned(),
                ),
                ..api("Marks every notification read", &[303], None)
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_push_route_is_the_notification_routes_under_the_api_prefix() {
        use navree_ui::{NativeClient, NativeFamily, NativeShellPart, NativeShellParts};
        for (prefix, route) in [
            ("/api", "/api/notifications/push-devices"),
            ("api/", "/api/notifications/push-devices"),
            ("", "/notifications/push-devices"),
            ("/v1/api", "/v1/api/notifications/push-devices"),
        ] {
            let mut parts = NativeShellParts::default();
            super::PushShellPart::new(prefix)
                .contribute(&mut parts, &NativeClient::new(NativeFamily::Android));
            assert_eq!(parts, {
                let mut said = NativeShellParts::default();
                said.push_register(route);
                said
            });
        }
    }

    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn item(data: serde_json::Value, read: bool) -> DatabaseNotification {
        DatabaseNotification {
            id: Uuid::nil(),
            notification_type: "contact.moved".to_owned(),
            data,
            read_at: read.then(Utc::now),
            created_at: "2026-11-06T10:30:00Z".parse().unwrap(),
        }
    }

    fn html(bell: NotificationBell) -> String {
        navree_ui::render(&bell.into())
    }

    #[test]
    fn the_bell_is_fetched_after_the_page_paints() {
        let page = html(NotificationBell::new());
        assert!(page.contains("data-ui-lazy"), "{page}");
        assert!(
            page.contains(r#"data-ui-src="/notifications/bell""#),
            "{page}"
        );
        assert!(page.contains("data-ui-poll"), "{page}");
    }

    #[test]
    fn it_lists_the_latest_with_a_count_and_links_only_to_this_site() {
        let items = vec![
            item(
                json!({ "title": "Contact moved", "message": "Gurpreet now works at Agra", "url": "/contacts/7" }),
                false,
            ),
            item(
                json!({ "title": "Off site", "url": "https://evil.example/x" }),
                false,
            ),
            item(json!({ "title": "Old news", "url": "/news" }), true),
        ];
        let page = html(NotificationBell::new().live().inbox(2, items));
        assert!(page.contains(r#"data-unread="2""#), "{page}");
        assert!(page.contains("Contact moved"), "{page}");
        assert!(page.contains("Gurpreet now works at Agra"), "{page}");
        assert!(page.contains(r#"href="/contacts/7""#), "{page}");
        assert!(!page.contains("evil.example"), "{page}");
        assert!(page.contains("Mark all read"), "{page}");
        assert!(page.contains("data-ui-live-src"), "{page}");
        // The read one has no "Mark read" of its own: two unread, two buttons, and the one for all.
        assert_eq!(page.matches("Mark read").count(), 2, "{page}");
        assert!(page.contains("6 Nov 2026, 10:30 UTC"), "{page}");
    }

    #[test]
    fn nothing_unread_has_no_count_and_an_empty_list_says_so() {
        let page = html(NotificationBell::new().inbox(0, Vec::new()));
        assert!(
            page.contains("You&#39;re all caught up") || page.contains("You're all caught up"),
            "{page}"
        );
        assert!(!page.contains("Mark all read"), "{page}");
        assert!(page.contains(r#"data-unread="0""#), "{page}");
    }
}
