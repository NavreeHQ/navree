//! [`StateKind`], what each kind of state is called, and [`StateLabels`], the words of
//! every state, in one place so another language is one value.

use std::borrow::Cow;

use http::StatusCode;

use crate::icons::{self, Icon};

/// What a state is about. [`StateKind::of`] reads it from a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StateKind {
    /// 401: not signed in.
    SignIn,
    /// 403: signed in, and not allowed.
    Forbidden,
    /// 404: nothing at this address.
    NotFound,
    /// 405: the address doesn't take this method.
    MethodNotAllowed,
    /// 409: the change clashes with another.
    Conflict,
    /// 410: it was here and is gone.
    Gone,
    /// 419, and a failed CSRF check: the page is too old to trust.
    Expired,
    /// 400 and 422: what was sent can't be used.
    Invalid,
    /// 429: too many requests.
    RateLimited,
    /// 500, and any other 5xx but 503: our failure.
    Failed,
    /// 503: busy or down on purpose.
    Unavailable,
    /// No answer at all: the connection failed. Only the browser knows this one.
    Offline,
    /// Any other 4xx.
    Client,
}

impl StateKind {
    /// Every kind, in the order labels are kept.
    pub const ALL: [Self; 13] = [
        Self::SignIn,
        Self::Forbidden,
        Self::NotFound,
        Self::MethodNotAllowed,
        Self::Conflict,
        Self::Gone,
        Self::Expired,
        Self::Invalid,
        Self::RateLimited,
        Self::Failed,
        Self::Unavailable,
        Self::Offline,
        Self::Client,
    ];

    /// The kind of an error with `status` and the framework's `code` (`csrf_token_mismatch`
    /// is an expired page whatever its status).
    pub fn of(status: StatusCode, code: &str) -> Self {
        if matches!(code, "csrf_token_mismatch" | "page_expired") {
            return Self::Expired;
        }
        match status.as_u16() {
            401 => Self::SignIn,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            405 => Self::MethodNotAllowed,
            409 => Self::Conflict,
            410 => Self::Gone,
            419 => Self::Expired,
            400 | 422 => Self::Invalid,
            429 => Self::RateLimited,
            503 => Self::Unavailable,
            500..=599 => Self::Failed,
            400..=499 => Self::Client,
            _ => Self::Failed,
        }
    }

    /// The status the kind stands for when nothing says otherwise.
    pub fn status(self) -> StatusCode {
        match self {
            Self::SignIn => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Gone => StatusCode::GONE,
            Self::Expired => StatusCode::from_u16(419).unwrap_or(StatusCode::BAD_REQUEST),
            Self::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Failed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Offline => StatusCode::BAD_GATEWAY,
            Self::Client => StatusCode::BAD_REQUEST,
        }
    }

    /// The name in `data-kind`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::SignIn => "sign-in",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::Conflict => "conflict",
            Self::Gone => "gone",
            Self::Expired => "expired",
            Self::Invalid => "invalid",
            Self::RateLimited => "rate-limited",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
            Self::Offline => "offline",
            Self::Client => "client",
        }
    }

    /// The kind called `name` in `data-kind`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The icon in the state's icon container.
    pub const fn icon(self) -> Icon {
        match self {
            Self::SignIn => icons::LOG_IN,
            Self::Forbidden => icons::SHIELD_ALERT,
            Self::NotFound => icons::FILE_SEARCH,
            Self::MethodNotAllowed => icons::BAN,
            Self::Conflict => icons::GIT_MERGE,
            Self::Gone => icons::FILE_X,
            Self::Expired => icons::TIMER,
            Self::Invalid => icons::CIRCLE_ALERT,
            Self::RateLimited => icons::GAUGE,
            Self::Failed => icons::SERVER_CRASH,
            Self::Unavailable => icons::WRENCH,
            Self::Offline => icons::WIFI_OFF,
            Self::Client => icons::TRIANGLE_ALERT,
        }
    }

    /// Whether the user can only wait or try again, as opposed to changing something.
    pub const fn is_transient(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Failed | Self::Unavailable | Self::Offline | Self::Expired
        )
    }

    /// Whether a region that gets this answer should stop asking: a door that is shut
    /// stays shut however often it is knocked on.
    pub const fn is_final(self) -> bool {
        matches!(
            self,
            Self::SignIn | Self::Forbidden | Self::NotFound | Self::Gone
        )
    }

    const fn index(self) -> usize {
        match self {
            Self::SignIn => 0,
            Self::Forbidden => 1,
            Self::NotFound => 2,
            Self::MethodNotAllowed => 3,
            Self::Conflict => 4,
            Self::Gone => 5,
            Self::Expired => 6,
            Self::Invalid => 7,
            Self::RateLimited => 8,
            Self::Failed => 9,
            Self::Unavailable => 10,
            Self::Offline => 11,
            Self::Client => 12,
        }
    }
}

/// The words of one kind of state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindWords {
    /// A short name: the page's title and its heading in the shell, and the name in a
    /// narrow place. "Page not found".
    pub short: Cow<'static, str>,
    /// The state's own headline, a plain sentence. "We can't find that page".
    pub title: Cow<'static, str>,
    /// What happened and what to do, in one or two short sentences.
    pub description: Cow<'static, str>,
    /// What a toast says when an action ends this way.
    pub toast: Cow<'static, str>,
}

fn words(
    short: &'static str,
    title: &'static str,
    description: &'static str,
    toast: &'static str,
) -> KindWords {
    KindWords {
        short: short.into(),
        title: title.into(),
        description: description.into(),
        toast: toast.into(),
    }
}

/// The words of every state. English by default; build your own for another language.
/// `{n}`, `{what}` and `{id}` are filled in.
///
/// ```
/// use navree_ui::pages::{StateKind, StateLabels};
///
/// let labels = StateLabels::default();
/// assert_eq!(labels.words(StateKind::NotFound).short, "Page not found");
/// ```
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct StateLabels {
    kinds: [KindWords; 13],
    /// "Go to home".
    pub home: Cow<'static, str>,
    /// "Sign in".
    pub sign_in: Cow<'static, str>,
    /// "Try again".
    pub try_again: Cow<'static, str>,
    /// "Reload".
    pub reload: Cow<'static, str>,
    /// "Request access".
    pub request_access: Cow<'static, str>,
    /// "Reference", before the request id.
    pub reference: Cow<'static, str>,
    /// "Copy reference": the name of the copy button.
    pub copy: Cow<'static, str>,
    /// "Try again in {n} seconds."
    pub wait: Cow<'static, str>,
    /// "Try again in 1 second."
    pub wait_one: Cow<'static, str>,
    /// "Development only", on the details a debug build adds.
    pub debug: Cow<'static, str>,
    /// "You don't have access to this {what}."
    pub forbidden_what: Cow<'static, str>,
    /// "This {what} doesn't exist, or it was removed."
    pub not_found_what: Cow<'static, str>,
    /// "You've been signed out", on the sign-in page after a session ended.
    pub signed_out: Cow<'static, str>,
    /// "Sign in again to carry on where you were."
    pub signed_out_help: Cow<'static, str>,
    /// "Sign in to see {what}." In a card or a panel that needs a user.
    pub sign_in_what: Cow<'static, str>,
    /// What a locked row says.
    pub row_locked: Cow<'static, str>,
    /// What a missing row says.
    pub row_missing: Cow<'static, str>,
    /// "Nothing here yet", the default headline of an empty list.
    pub nothing_yet: Cow<'static, str>,
    /// "No {what} yet".
    pub nothing_yet_what: Cow<'static, str>,
    /// "No results", when a search or filter finds nothing.
    pub no_results: Cow<'static, str>,
    /// "Nothing matches your search or filters. Change them, or clear them to see everything."
    pub no_results_help: Cow<'static, str>,
    /// "Clear filters".
    pub clear_filters: Cow<'static, str>,
    /// "You're all caught up".
    pub caught_up: Cow<'static, str>,
    /// "There is nothing left to do here."
    pub caught_up_help: Cow<'static, str>,
}

impl Default for StateLabels {
    fn default() -> Self {
        Self {
            kinds: [
                words(
                    "Sign in required",
                    "Sign in to continue",
                    "You need to be signed in to see this. After you sign in, you come back here.",
                    "Your session ended. Sign in to continue.",
                ),
                words(
                    "No access",
                    "You don't have access to this",
                    "Your account can't open this page. If you think it should, ask an admin to give you access.",
                    "You don't have permission to do that.",
                ),
                words(
                    "Page not found",
                    "We can't find that page",
                    "The address may be wrong, or the page may have moved or been deleted.",
                    "That no longer exists.",
                ),
                words(
                    "Not allowed",
                    "That isn't allowed here",
                    "This address doesn't take that kind of request. Go to the home page and start again.",
                    "That action isn't available here.",
                ),
                words(
                    "Conflict",
                    "That clashes with another change",
                    "Someone changed this while you were working on it. Reload to see the latest version, then try again.",
                    "That clashes with a change made elsewhere. Reload and try again.",
                ),
                words(
                    "Gone",
                    "This is gone for good",
                    "The page you asked for was removed and won't be back.",
                    "That was removed and won't be back.",
                ),
                words(
                    "Page expired",
                    "This page expired",
                    "It was open for a long time, or your session changed. Reload the page and try again.",
                    "This page expired. Reload it and try again.",
                ),
                words(
                    "Couldn't use that",
                    "We couldn't use what you sent",
                    "Some of it isn't valid. Check it and try again.",
                    "That wasn't valid. Check it and try again.",
                ),
                words(
                    "Too many requests",
                    "You're going too fast",
                    "Wait a moment, then try again.",
                    "Too many requests. Wait a moment and try again.",
                ),
                words(
                    "Something went wrong",
                    "Something went wrong on our side",
                    "That wasn't your doing. Try again in a moment. If it keeps happening, tell support the reference below.",
                    "Something went wrong on our side. Try again in a moment.",
                ),
                words(
                    "Unavailable",
                    "We'll be right back",
                    "The service is busy or down for maintenance. Try again in a few minutes.",
                    "The service is unavailable. Try again in a few minutes.",
                ),
                words(
                    "No connection",
                    "You're offline",
                    "Check your connection and try again.",
                    "No connection. Try again.",
                ),
                words(
                    "Request problem",
                    "That request didn't work",
                    "Check what you asked for and try again.",
                    "That didn't work. Try again.",
                ),
            ],
            home: "Go to home".into(),
            sign_in: "Sign in".into(),
            try_again: "Try again".into(),
            reload: "Reload".into(),
            request_access: "Request access".into(),
            reference: "Reference".into(),
            copy: "Copy reference".into(),
            wait: "Try again in {n} seconds.".into(),
            wait_one: "Try again in 1 second.".into(),
            debug: "Development only".into(),
            forbidden_what: "You don't have access to this {what}.".into(),
            not_found_what: "This {what} doesn't exist, or it was removed.".into(),
            signed_out: "You've been signed out".into(),
            signed_out_help: "Sign in again to carry on where you were.".into(),
            sign_in_what: "Sign in to see {what}.".into(),
            row_locked: "You no longer have access to this row.".into(),
            row_missing: "This row was removed.".into(),
            nothing_yet: "Nothing here yet".into(),
            nothing_yet_what: "No {what} yet".into(),
            no_results: "No results".into(),
            no_results_help:
                "Nothing matches your search or filters. Change them, or clear them to see everything."
                    .into(),
            clear_filters: "Clear filters".into(),
            caught_up: "You're all caught up".into(),
            caught_up_help: "There is nothing left to do here.".into(),
        }
    }
}

impl StateLabels {
    /// The words of `kind`.
    pub fn words(&self, kind: StateKind) -> &KindWords {
        &self.kinds[kind.index()]
    }

    /// Replaces the words of `kind`.
    #[must_use]
    pub fn with_words(mut self, kind: StateKind, words: KindWords) -> Self {
        self.kinds[kind.index()] = words;
        self
    }

    /// "Try again in 12 seconds." for `seconds`, read from `Retry-After`.
    pub fn wait_text(&self, seconds: u64) -> String {
        if seconds == 1 {
            self.wait_one.to_string()
        } else {
            self.wait.replace("{n}", &seconds.to_string())
        }
    }

    /// `template` with `{what}` filled in.
    pub fn about(template: &str, what: &str) -> String {
        template.replace("{what}", what)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_its_kind() {
        let cases = [
            (401, "unauthenticated", StateKind::SignIn),
            (403, "forbidden", StateKind::Forbidden),
            (403, "demo_read_only", StateKind::Forbidden),
            (403, "csrf_token_mismatch", StateKind::Expired),
            (404, "not_found", StateKind::NotFound),
            (405, "method_not_allowed", StateKind::MethodNotAllowed),
            (409, "conflict", StateKind::Conflict),
            (409, "limit_reached", StateKind::Conflict),
            (410, "gone", StateKind::Gone),
            (419, "page_expired", StateKind::Expired),
            (419, "", StateKind::Expired),
            (422, "", StateKind::Invalid),
            (400, "validation_failed", StateKind::Invalid),
            (429, "too_many_requests", StateKind::RateLimited),
            (500, "internal_error", StateKind::Failed),
            (502, "", StateKind::Failed),
            (504, "", StateKind::Failed),
            (503, "maintenance", StateKind::Unavailable),
            (503, "request_timeout", StateKind::Unavailable),
            (413, "payload_too_large", StateKind::Client),
            (415, "", StateKind::Client),
            (418, "", StateKind::Client),
            (200, "", StateKind::Failed),
        ];
        for (status, code, kind) in cases {
            let status = StatusCode::from_u16(status).unwrap();
            assert_eq!(StateKind::of(status, code), kind, "{status} {code}");
        }
    }

    #[test]
    fn every_kind_has_words_a_name_and_an_icon() {
        let labels = StateLabels::default();
        let mut names = std::collections::HashSet::new();
        for kind in StateKind::ALL {
            let words = labels.words(kind);
            for text in [&words.short, &words.title, &words.description, &words.toast] {
                assert!(!text.is_empty(), "{kind:?}");
                assert!(!text.contains("{"), "{kind:?}: {text}");
            }
            assert!(names.insert(kind.name()), "{kind:?} repeats a name");
            assert_eq!(StateKind::from_name(kind.name()), Some(kind));
            assert!(!kind.icon().shapes().is_empty());
        }
        assert_eq!(StateKind::from_name("nope"), None);
        assert_eq!(StateKind::ALL.len(), names.len());
    }

    #[test]
    fn indexes_are_distinct_and_in_order() {
        for (at, kind) in StateKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), at);
        }
    }

    #[test]
    fn the_wait_reads_as_a_sentence() {
        let labels = StateLabels::default();
        assert_eq!(labels.wait_text(1), "Try again in 1 second.");
        assert_eq!(labels.wait_text(12), "Try again in 12 seconds.");
    }

    #[test]
    fn a_closed_door_stays_closed() {
        for kind in [
            StateKind::SignIn,
            StateKind::Forbidden,
            StateKind::NotFound,
            StateKind::Gone,
        ] {
            assert!(kind.is_final(), "{kind:?}");
        }
        for kind in [
            StateKind::Failed,
            StateKind::RateLimited,
            StateKind::Unavailable,
        ] {
            assert!(!kind.is_final() && kind.is_transient(), "{kind:?}");
        }
    }

    #[test]
    fn the_words_of_a_kind_can_be_replaced() {
        let labels = StateLabels::default().with_words(
            StateKind::NotFound,
            KindWords {
                short: "Introuvable".into(),
                title: "Page introuvable".into(),
                description: "Cette page n'existe pas.".into(),
                toast: "Introuvable.".into(),
            },
        );
        assert_eq!(labels.words(StateKind::NotFound).short, "Introuvable");
        assert_eq!(labels.words(StateKind::Forbidden).short, "No access");
    }
}
