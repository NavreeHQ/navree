//! What a control starts on the device of a native app, in place of what it does in a browser.
//!
//! A page is written once. Where the app a native client runs in can do with the system's
//! own sheet what a browser does on a page (a passkey, Sign in with Apple or with Google),
//! the control says so with [`Element::native_call`](crate::Element::native_call), as plain
//! data: a [`NativeCall`]. A native client whose app says it has the feature gets the call
//! on the control; every other client, and a browser, gets the control as it was.
//!
//! The routes a call names belong to the module that writes it (authentication, social
//! sign-in), which checks there everything the call brings back. Nothing here depends on
//! the preview feature: a module says its calls the same whether or not an app answers
//! native clients, and the preview reads them.
//!
//! ```
//! use navree_ui::{NativeCall, NativeProvider, el, native_client_with, render};
//!
//! let call = NativeCall::provider_sign_in(
//!     NativeProvider::Apple,
//!     "/api/auth/social/apple/native/nonce",
//!     "/api/auth/social/apple/native/session",
//! );
//! // A browser's page is the page it was.
//! let link = el("a").href("/sign-in/apple").child("Sign in with Apple").native_call(call);
//! assert_eq!(render(&link.into()), r#"<a href="/sign-in/apple">Sign in with Apple</a>"#);
//! // Outside a native client's request, no app can start anything.
//! assert!(native_client_with("platform_sign_in").is_none());
//! ```

use std::fmt;

use crate::shell::NativeClient;

/// Who a sign-in in the system's own sheet is with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeProvider {
    /// Sign in with Apple: Apple's systems have a sheet for it.
    Apple,
    /// Sign in with Google: Android has a sheet for it.
    Google,
}

/// Where a device call's result is posted: a route of this site, with the sealed one-time
/// ticket the server wrote for the call (navree-contracts' `CallTickets`) and when it stops
/// being good. The client can carry the ticket and can't read, make or change one, so its
/// text is never shown by `Debug`.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeThen {
    /// The route the result is posted to, as a form: a path on this site, with its query.
    pub href: String,
    /// The sealed ticket the post carries back as `_call`.
    pub ticket: String,
    /// When the ticket expires, in seconds since 1970 (UTC).
    pub expires_at: i64,
}

impl NativeThen {
    /// A result posted to `href` with `ticket`, which is good until `expires_at`.
    pub fn new(href: impl Into<String>, ticket: impl Into<String>, expires_at: i64) -> Self {
        Self {
            href: href.into(),
            ticket: ticket.into(),
            expires_at,
        }
    }
}

impl fmt::Debug for NativeThen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeThen")
            .field("href", &self.href)
            .field("ticket", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// What a call is, with the routes it speaks to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    PasskeySignIn {
        options: String,
        then: NativeThen,
    },
    ProviderSignIn {
        provider: NativeProvider,
        nonce: String,
        sign_in: String,
        client_id: Option<String>,
        second_step: Option<String>,
    },
}

/// A call a person's tap starts on the device of a native app: see the
/// [module](crate::device). Every path is one on this site; a call with a path that is not
/// is never sent.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct NativeCall(Kind);

impl NativeCall {
    /// Signs in with a passkey in the system's own sheet (the protocol's `passkey` call).
    ///
    /// On a tap the client posts `{}` as JSON to `options`, which answers the request's
    /// options as WebAuthn writes them (`{"publicKey": {...}}`); it shows the system's
    /// sheet, then posts the assertion to `then` as a form: `credential` (WebAuthn's
    /// `toJSON()`, as text), the ticket as `_call` and `outcome` (`ok`, or why there is no
    /// assertion). Both posts carry the session's cookie and its token as `X-XSRF-TOKEN`.
    ///
    /// It is written for a client whose app lists `passkey` and `passkey_options`: a build
    /// that lists `passkey` alone is from before the options were handed to it, and gets
    /// the control as it was.
    pub fn passkey_sign_in(options: impl Into<String>, then: NativeThen) -> Self {
        Self(Kind::PasskeySignIn {
            options: options.into(),
            then,
        })
    }

    /// Signs in with Apple or Google in the system's own sheet (the protocol's
    /// `platform_sign_in` call).
    ///
    /// On a tap the client posts to `nonce` (JSON, no body), which answers
    /// `{"nonce", "state", "expires_in"}`; it shows the provider's sheet with the nonce,
    /// then posts the identity token and the state to `sign_in` as JSON. The call has no
    /// ticket: the sealed state, used once, stands for it.
    pub fn provider_sign_in(
        provider: NativeProvider,
        nonce: impl Into<String>,
        sign_in: impl Into<String>,
    ) -> Self {
        Self(Kind::ProviderSignIn {
            provider,
            nonce: nonce.into(),
            sign_in: sign_in.into(),
            client_id: None,
            second_step: None,
        })
    }

    /// The app's client id with the provider, where its sheet needs one: Google's server
    /// client id on Android. Nothing for a passkey.
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        if let Kind::ProviderSignIn { client_id, .. } = &mut self.0 {
            *client_id = Some(id.into());
        }
        self
    }

    /// The page of this site that finishes a second step of the sign-in (a code from an
    /// authenticator app), in place of the one the shell document names. Nothing for a
    /// passkey, whose route leads there itself.
    pub fn second_step(mut self, path: impl Into<String>) -> Self {
        if let Kind::ProviderSignIn { second_step, .. } = &mut self.0 {
            *second_step = Some(path.into());
        }
        self
    }

    /// The device feature the call belongs to, as a client's capabilities name it:
    /// `passkey` or `platform_sign_in`. Ask [`native_client_with`] whether the client of
    /// the request can start it.
    pub fn feature(&self) -> &'static str {
        match self.0 {
            Kind::PasskeySignIn { .. } => PASSKEY,
            Kind::ProviderSignIn { .. } => "platform_sign_in",
        }
    }
}

/// The feature a passkey call belongs to.
const PASSKEY: &str = "passkey";

/// The native client the current request is from, when its app says it can do the device
/// feature `feature` (`passkey`, `platform_sign_in`: the names of the protocol's
/// catalogue). `None` for a browser, for the web view inside a native client, for a native
/// client whose app does not list the feature among its capabilities, and always without
/// the `native-preview` feature.
///
/// For `passkey` the app must also list `passkey_options`, which says its build shows the
/// system's sheet with the options the server gave. A build from before that lists
/// `passkey` alone and can only fail the call, so it is answered here as a client without
/// passkeys, and no passkey call is written for it.
///
/// For the page that draws a control only where it can be used, and that makes what the
/// call needs (a ticket) only then. A capability is the client's own word: it decides what
/// is offered, never what is accepted.
#[cfg_attr(not(feature = "native-preview"), allow(unused_variables))]
pub fn native_client_with(feature: &str) -> Option<NativeClient> {
    #[cfg(feature = "native-preview")]
    {
        let request = crate::native::scope::native()?;
        can_start(&request.client, feature)
            .then(|| NativeClient::new(family_of(request.client.platform)))
    }
    #[cfg(not(feature = "native-preview"))]
    {
        None
    }
}

/// Whether the app of `client` can carry out a call of `feature` as this server writes
/// it: it lists the feature, and for a passkey also the name that says its build takes
/// the request's options from the runtime core
/// ([`PASSKEY_OPTIONS_CAP`](navree_native::device::PASSKEY_OPTIONS_CAP)).
#[cfg(feature = "native-preview")]
fn can_start(client: &navree_native::Client, feature: &str) -> bool {
    client.supports(feature)
        && (feature != PASSKEY || client.supports(navree_native::device::PASSKEY_OPTIONS_CAP))
}

/// The family of systems `platform` belongs to.
#[cfg(feature = "native-preview")]
pub(crate) fn family_of(platform: navree_native::Platform) -> crate::shell::NativeFamily {
    use crate::shell::NativeFamily;
    use navree_native::Platform as P;
    match platform {
        P::Ios | P::Ipados | P::Macos | P::Watchos | P::Tvos | P::Visionos => NativeFamily::Apple,
        P::Android | P::Wearos => NativeFamily::Android,
        _ => NativeFamily::Other,
    }
}

#[cfg(feature = "native-preview")]
impl NativeCall {
    /// The call as the protocol writes it, for the native client the current request is
    /// from. `None` when no native client asks, when its app can't start the call (it
    /// does not list the call's feature among its capabilities, or, for a passkey, its
    /// build is from before the options were handed over), and when a path of the call is
    /// not one on this site.
    pub(crate) fn for_the_client(&self) -> Option<navree_native::DeviceCall> {
        use navree_native::device::{
            DeviceCall, OriginPath, Passkey, PasskeyMode, PlatformSignIn, SignInProvider, Then,
            Ticket,
        };
        let request = crate::native::scope::native()?;
        if !can_start(&request.client, self.feature()) {
            return None;
        }
        let path = |text: &str| OriginPath::parse(text).ok();
        Some(match &self.0 {
            Kind::PasskeySignIn { options, then } => {
                // A ticket the protocol would not carry is not sent.
                if then.ticket.is_empty() || then.ticket.len() > Ticket::MAX {
                    return None;
                }
                DeviceCall::new(Passkey {
                    mode: PasskeyMode::SignIn,
                    options: path(options)?,
                    conditional: false,
                })
                .then(Then {
                    href: path(&then.href)?,
                    ticket: Ticket::from(then.ticket.clone()),
                    exp: then.expires_at,
                })
            }
            Kind::ProviderSignIn {
                provider,
                nonce,
                sign_in,
                client_id,
                second_step,
            } => DeviceCall::new(PlatformSignIn {
                provider: match provider {
                    NativeProvider::Apple => SignInProvider::Apple,
                    NativeProvider::Google => SignInProvider::Google,
                },
                nonce: path(nonce)?,
                sign_in: path(sign_in)?,
                web: None,
                client_id: client_id.clone(),
                second_step: match second_step {
                    Some(page) => Some(path(page)?),
                    None => None,
                },
            }),
        })
    }
}
