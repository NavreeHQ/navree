//! Who asked: the client of the request the current task works for.
//!
//! The reply layer reads the request's headers once and makes the answer
//! current for the handler it runs, so the places that turn a tree into bytes
//! can ask without being handed anything. It is a task-local value: a spawned
//! task does not inherit it, and outside a request there is none.

use std::future::Future;
use std::sync::Arc;

use http::HeaderMap;
use http::request::Parts;
use navree_http::extract::FromRequestParts;
use navree_native::{CLIENT_HEADER, ClientError, FormFactor, NATIVE_HEADER, Platform};

use crate::asked::{WhoAsked, in_web_view, who_asked};

tokio::task_local! {
    static ASKED: Asked;
}

/// What the request asked to be answered as, when not as a plain web page.
#[derive(Clone, Debug)]
pub(crate) enum Asked {
    /// A native client: answer with an envelope.
    Native(Arc<Request>),
    /// The web view inside a native client: answer with HTML, without the
    /// app's chrome.
    Embed,
}

/// A native client's request.
#[derive(Debug)]
pub(crate) struct Request {
    /// The client.
    pub(crate) client: navree_native::Client,
    /// The address it asked for, with its query: where its web view goes when
    /// the page can't be drawn natively.
    pub(crate) url: String,
}

/// What a request's headers ask for, or why they can't be answered.
pub(crate) enum Reading {
    /// A browser: the headers say nothing.
    Web,
    /// A native client or its web view.
    Asked(Asked),
    /// A native client whose protocol this server does not speak.
    UnknownMajor(String),
    /// A native client whose descriptor can't be read.
    BadClient(ClientError),
}

/// Reads what `headers` ask for, by [`who_asked`]: a native client, the web
/// view inside one (its runtime's requests too), or the web.
pub(crate) fn read(headers: &HeaderMap, url: &str) -> Reading {
    match who_asked(headers) {
        WhoAsked::Native => {}
        WhoAsked::WebView => return Reading::Asked(Asked::Embed),
        WhoAsked::Runtime if in_web_view(headers) => return Reading::Asked(Asked::Embed),
        _ => return Reading::Web,
    }
    let Some(major) = headers.get(NATIVE_HEADER) else {
        return Reading::Web;
    };
    let major = major.to_str().unwrap_or("").trim();
    if major.parse::<u32>().ok() != Some(navree_native::MAJOR) {
        return Reading::UnknownMajor(major.to_owned());
    }
    let descriptor = headers
        .get(CLIENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    match navree_native::Client::parse(descriptor) {
        Ok(client) => Reading::Asked(Asked::Native(Arc::new(Request {
            client,
            url: url.to_owned(),
        }))),
        Err(error) => Reading::BadClient(error),
    }
}

/// Runs `future` with `asked` current.
pub(crate) async fn scope<F: Future>(asked: Asked, future: F) -> F::Output {
    ASKED.scope(asked, future).await
}

/// Runs `build` with `asked` current, for a page built outside a request: a test.
pub(crate) fn sync<T>(asked: Asked, build: impl FnOnce() -> T) -> T {
    ASKED.sync_scope(asked, build)
}

/// What the current request asked for, when a reply layer is around it and it
/// did not come from a browser.
#[inline]
pub(crate) fn asked() -> Option<Asked> {
    ASKED.try_with(Clone::clone).ok()
}

/// The native client's request the current task works for, if it is one.
#[inline]
pub(crate) fn native() -> Option<Arc<Request>> {
    ASKED
        .try_with(|asked| match asked {
            Asked::Native(request) => Some(request.clone()),
            Asked::Embed => None,
        })
        .ok()
        .flatten()
}

/// Who asked: a native client with its system and form factor, or a browser.
///
/// An extractor, like any other, for the handler whose screen really differs
/// from one system to another. Most handlers never take it: a page is written
/// once, and each client places its parts as its own system does.
///
/// ```ignore
/// async fn show(client: Client, shell: AppShell) -> AppPage {
///     let page = shell.page("Scan");
///     if client.supports("camera") {
///         page.body(Button::primary("Scan a code").href("/scan"))
///     } else {
///         page.body(Text::new("Type the code from the label."))
///     }
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct Client(Option<Arc<Request>>);

impl Client {
    /// The client of the request the current task works for. A browser's when
    /// there is no request, or no reply layer around it.
    pub fn current() -> Self {
        Self(native())
    }

    /// A native client, for a test.
    pub fn of(client: navree_native::Client) -> Self {
        Self(Some(Arc::new(Request {
            client,
            url: String::new(),
        })))
    }

    /// Whether a native client asked.
    pub fn is_native(&self) -> bool {
        self.0.is_some()
    }

    /// What the client said about itself, when it is a native one.
    pub fn descriptor(&self) -> Option<&navree_native::Client> {
        self.0.as_deref().map(|request| &request.client)
    }

    /// The client's system, when it is a native one.
    pub fn platform(&self) -> Option<Platform> {
        self.descriptor().map(|client| client.platform)
    }

    /// The kind of device it draws on, when it is a native one.
    pub fn form(&self) -> Option<FormFactor> {
        self.descriptor().map(|client| client.form)
    }

    /// Whether it is a native client that draws one narrow column.
    pub fn is_compact(&self) -> bool {
        self.descriptor()
            .is_some_and(navree_native::Client::is_compact)
    }

    /// Whether it is a native client whose app may use the device feature `cap`.
    pub fn supports(&self, cap: &str) -> bool {
        self.descriptor().is_some_and(|client| client.supports(cap))
    }
}

impl FromRequestParts for Client {
    type Rejection = std::convert::Infallible;

    /// Reads the request's own headers, so it answers the same with or without
    /// the reply layer. A descriptor that can't be read is a browser's here; the
    /// layer refuses such a request before a handler sees it.
    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let url = parts.uri.path_and_query().map_or_else(
            || parts.uri.path().to_owned(),
            |url| url.as_str().to_owned(),
        );
        Ok(match read(&parts.headers, &url) {
            Reading::Asked(Asked::Native(request)) => Self(Some(request)),
            _ => Self(None),
        })
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn a_browser_asks_for_nothing() {
        assert!(matches!(read(&HeaderMap::new(), "/"), Reading::Web));
        assert!(matches!(
            read(&headers(&[("x-navree-ui", "1")]), "/"),
            Reading::Web
        ));
    }

    #[test]
    fn a_native_client_is_read_with_the_address_it_asked_for() {
        let asked = read(
            &headers(&[
                (NATIVE_HEADER, "1"),
                (CLIENT_HEADER, "platform=android; form=phone; caps=camera"),
            ]),
            "/orders?page=2",
        );
        let Reading::Asked(Asked::Native(request)) = asked else {
            panic!("not a native request");
        };
        assert_eq!(request.client.platform, Platform::Android);
        assert_eq!(request.url, "/orders?page=2");
    }

    #[test]
    fn a_major_or_a_descriptor_the_server_can_not_read_is_said() {
        assert!(matches!(
            read(&headers(&[(NATIVE_HEADER, "2")]), "/"),
            Reading::UnknownMajor(major) if major == "2"
        ));
        assert!(matches!(
            read(&headers(&[(NATIVE_HEADER, "1")]), "/"),
            Reading::BadClient(ClientError::NoPlatform)
        ));
        assert!(matches!(
            read(
                &headers(&[
                    (NATIVE_HEADER, "1"),
                    (CLIENT_HEADER, "platform=beos; form=phone")
                ]),
                "/"
            ),
            Reading::BadClient(ClientError::UnknownPlatform(_))
        ));
    }

    #[test]
    fn the_web_view_of_a_native_client_asks_for_a_page_without_chrome() {
        assert!(matches!(
            read(&headers(&[(navree_native::EMBED_HEADER, "1")]), "/"),
            Reading::Asked(Asked::Embed)
        ));
        let agent = "Mozilla/5.0 (Linux; Android 16; wv) Chrome/140 Mobile Safari/537.36 NavreeNative/0.1.0";
        for pairs in [
            &[("user-agent", agent)][..],
            &[("user-agent", agent), ("x-navree-ui", "1")][..],
        ] {
            assert!(matches!(
                read(&headers(pairs), "/"),
                Reading::Asked(Asked::Embed)
            ));
        }
    }

    #[tokio::test]
    async fn the_client_is_current_inside_its_scope_and_nowhere_else() {
        assert!(!Client::current().is_native());
        assert!(asked().is_none());
        let Reading::Asked(native) = read(
            &headers(&[
                (NATIVE_HEADER, "1"),
                (CLIENT_HEADER, "platform=ios; form=phone; caps=camera"),
            ]),
            "/",
        ) else {
            panic!("not a native request");
        };
        scope(native, async {
            let client = Client::current();
            assert!(client.is_native());
            assert_eq!(client.platform(), Some(Platform::Ios));
            assert_eq!(client.form(), Some(FormFactor::Phone));
            assert!(client.is_compact());
            assert!(client.supports("camera"));
            assert!(!client.supports("push"));
            // Across an await, on the same task.
            tokio::task::yield_now().await;
            assert!(Client::current().is_native());
            // A spawned task is another task.
            let spawned = tokio::spawn(async { Client::current().is_native() });
            assert!(!spawned.await.unwrap());
        })
        .await;
        scope(Asked::Embed, async {
            assert!(!Client::current().is_native());
            assert!(matches!(asked(), Some(Asked::Embed)));
        })
        .await;
        assert!(!Client::current().is_native());
    }
}
