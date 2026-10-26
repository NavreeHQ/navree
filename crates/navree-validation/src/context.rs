//! [`InputContext`] and [`RegionSource`]: what the international types read
//! while the extractors deserialize and validate.
//!
//! The extractors resolve the context lazily: they first run without it,
//! and only when a value asks for it (a phone number written without a
//! country code, a `SafeHtml` field) do they resolve it, asking the app's
//! [`RegionSource`] once, and run again. Input without such values costs
//! nothing extra.

use std::cell::RefCell;
use std::sync::{Arc, LazyLock};

use http::request::Parts;
use navree_kernel::{AppContext, BoxFuture};

use crate::{Country, HtmlPolicy};

/// What the international types read while deserializing and validating:
/// the default region of phone numbers written without a country code, and
/// the policy `SafeHtml` sanitizes with.
///
/// The extractors build it per request: `ValidationModule` provides the
/// app's (`validation.phone.default_region`, `validation.html`), and the
/// app's [`RegionSource`], if any, picks the request's region. Without the
/// module the defaults apply: no default region and the default
/// [`HtmlPolicy`].
///
/// Outside the extractors (a job deserializing its payload), run the code
/// in [`scope`](InputContext::scope) to use the app's context; without one
/// the defaults apply.
///
/// ```
/// use navree_validation::{Country, InputContext, Locale};
///
/// let context = InputContext::new().with_default_region(Country::parse("GB").ok());
/// let locale: Locale = context.scope(|| serde_json::from_str(r#""en-gb""#)).unwrap();
/// assert_eq!(locale.as_str(), "en-GB");
/// assert_eq!(context.default_region().map(|c| c.as_str()), Some("GB"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputContext {
    default_region: Option<Country>,
    html: Arc<HtmlPolicy>,
}

impl InputContext {
    /// The defaults: no default region, the default [`HtmlPolicy`].
    pub fn new() -> Self {
        Self::default()
    }

    /// With `region` as the default region of phone numbers.
    pub fn with_default_region(mut self, region: Option<Country>) -> Self {
        self.default_region = region;
        self
    }

    /// With `policy` for `SafeHtml`.
    pub fn with_html(mut self, policy: HtmlPolicy) -> Self {
        self.html = Arc::new(policy);
        self
    }

    /// The region of phone numbers written without a country code.
    pub fn default_region(&self) -> Option<Country> {
        self.default_region
    }

    /// The policy `SafeHtml` sanitizes with.
    pub fn html(&self) -> &HtmlPolicy {
        &self.html
    }

    /// Runs `f` with this context: values deserialized or validated inside
    /// it use its region and policy.
    pub fn scope<R>(&self, f: impl FnOnce() -> R) -> R {
        let _restore = enter(State::Ready(self.clone()));
        f()
    }

    /// The context of a request: the app's (or the defaults), with the
    /// region of the app's [`RegionSource`] when it gives one.
    pub(crate) async fn for_request(app: Option<&AppContext>, parts: &mut Parts) -> InputContext {
        let Some(app) = app else {
            return defaults().clone();
        };
        let mut context = match app.try_get::<InputContext>().await {
            Some(context) => InputContext::clone(&context),
            None => defaults().clone(),
        };
        if let Some(source) = app.try_get::<dyn RegionSource>().await
            && let Some(region) = source.region(parts).await
        {
            context.default_region = Some(region);
        }
        context
    }
}

/// Picks the default region of a request's phone numbers: the tenant's
/// country, the user's, a header set by the edge. It is asked only when a
/// number is written without a country code, at most once per request;
/// `None` falls back to `validation.phone.default_region`.
///
/// Register one with
/// [`ModuleBuilderValidationExt::region_source`](crate::ModuleBuilderValidationExt::region_source)
/// (or as a `dyn RegionSource` provider).
///
/// ```
/// use http::request::Parts;
/// use navree_kernel::BoxFuture;
/// use navree_validation::{Country, RegionSource};
///
/// /// The country the edge proxy puts in `CF-IPCountry`.
/// struct EdgeCountry;
///
/// impl RegionSource for EdgeCountry {
///     fn region<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Country>> {
///         let country = parts
///             .headers
///             .get("cf-ipcountry")
///             .and_then(|value| value.to_str().ok())
///             .and_then(|code| Country::parse(code).ok());
///         Box::pin(async move { country })
///     }
/// }
/// ```
pub trait RegionSource: Send + Sync + 'static {
    /// The default region for the request whose head is `parts` (its
    /// extensions hold the principal and anything earlier extractors
    /// cached), or `None` for the configured one.
    fn region<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Option<Country>>;
}

/// The shared defaults (outside the extractors, or without the module).
fn defaults() -> &'static InputContext {
    static DEFAULTS: LazyLock<InputContext> = LazyLock::new(InputContext::default);
    &DEFAULTS
}

/// Where the current thread is.
enum State {
    /// Neither in an extractor nor in a scope: the defaults apply.
    Outside,
    /// In an extractor that has not resolved the context: a value that
    /// needs it asks (and fails), and the extractor runs again with it.
    Pending { asked: bool },
    /// In an extractor or a scope with its context.
    Ready(InputContext),
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Outside) };
}

/// Restores the previous state on drop, panics included.
struct Restore(Option<State>);

impl Drop for Restore {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            STATE.with(|state| *state.borrow_mut() = previous);
        }
    }
}

fn enter(state: State) -> Restore {
    Restore(Some(STATE.with(|current| current.replace(state))))
}

/// The context in effect: the scope's, the defaults outside any, or `None`
/// in an extractor that has not resolved it yet (the caller fails; the
/// extractor runs again with the context).
pub(crate) fn current() -> Option<InputContext> {
    STATE.with(|state| match &mut *state.borrow_mut() {
        State::Outside => Some(defaults().clone()),
        State::Pending { asked } => {
            *asked = true;
            None
        }
        State::Ready(context) => Some(context.clone()),
    })
}

/// Runs `f` without a resolved context; `true` when a value asked for it,
/// in which case the caller resolves it and runs `f` again in its scope.
pub(crate) fn lazily<R>(f: impl FnOnce() -> R) -> (R, bool) {
    let restore = enter(State::Pending { asked: false });
    let result = f();
    let asked = STATE.with(|state| matches!(*state.borrow(), State::Pending { asked: true }));
    drop(restore);
    (result, asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> Option<&'static str> {
        current().and_then(|context| context.default_region().map(|c| c.as_str()))
    }

    #[test]
    fn scopes_nest_and_restore() {
        assert_eq!(region(), None);
        let gb = InputContext::new().with_default_region(Country::parse("GB").ok());
        let ae = InputContext::new().with_default_region(Country::parse("AE").ok());
        gb.scope(|| {
            assert_eq!(region(), Some("GB"));
            ae.scope(|| assert_eq!(region(), Some("AE")));
            assert_eq!(region(), Some("GB"));
        });
        assert_eq!(region(), None);

        let caught = std::panic::catch_unwind(|| gb.scope(|| panic!("inside")));
        assert!(caught.is_err());
        assert_eq!(region(), None);
    }

    #[test]
    fn lazily_reports_whether_the_context_was_asked_for() {
        let ((), asked) = lazily(|| ());
        assert!(!asked);
        let (seen, asked) = lazily(current);
        assert!(seen.is_none());
        assert!(asked);
        // Back outside: the defaults.
        assert_eq!(current(), Some(InputContext::default()));
        // A scope inside wins.
        let gb = InputContext::new().with_default_region(Country::parse("GB").ok());
        let (seen, asked) = lazily(|| gb.scope(region));
        assert_eq!(seen, Some("GB"));
        assert!(!asked);
    }
}
