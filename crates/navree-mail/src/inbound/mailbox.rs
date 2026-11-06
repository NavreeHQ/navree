//! Mailboxes: address patterns, their handlers and
//! [`ModuleBuilderMailExt`].

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, ModuleBuilder, ModuleId, RequestScope,
};
use regex::Regex;
use uuid::Uuid;

use super::error::InboundError;
use super::mail::InboundMail;
use super::tokens::ReplyTokens;

/// An address pattern: `support@acme.test`, `reply+{token}@in.acme.test`,
/// `*@in.acme.test` or `*`.
///
/// - Letters compare without case; the rest of the address must match
///   exactly.
/// - `{name}` captures one or more characters before the `@` (or of the
///   domain, after it), read with [`MailboxContext::param`].
/// - `*` matches any run of characters except `@`; a pattern that is only
///   `*` matches every address.
///
/// Mailboxes are tried most specific first: exact addresses, then patterns
/// with `{name}`, then patterns with `*`, each group in registration order.
#[derive(Clone)]
pub struct MailboxPattern {
    pattern: String,
    regex: Regex,
    params: Vec<String>,
    rank: u8,
}

impl MailboxPattern {
    /// Parses `pattern`.
    ///
    /// # Errors
    ///
    /// [`InboundError::InvalidPattern`] for an empty pattern, an
    /// unclosed or empty `{}`, a repeated name, or a pattern with no `@`
    /// (other than `*`).
    pub fn parse(pattern: &str) -> Result<Self, InboundError> {
        let invalid = |reason: &str| InboundError::InvalidPattern {
            pattern: pattern.to_owned(),
            reason: reason.to_owned(),
        };
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Err(invalid("the pattern is empty"));
        }
        if trimmed != "*" && trimmed.matches('@').count() != 1 {
            return Err(invalid("a pattern needs one `@` (or is `*`)"));
        }
        let mut regex = String::from("(?i)^");
        let mut params: Vec<String> = Vec::new();
        let mut wildcard = false;
        let mut chars = trimmed.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(c) if c.is_ascii_alphanumeric() || c == '_' => name.push(c),
                            Some(_) => {
                                return Err(invalid(
                                    "a `{name}` holds letters, digits and underscores",
                                ));
                            }
                            None => return Err(invalid("a `{` is not closed")),
                        }
                    }
                    if name.is_empty() {
                        return Err(invalid("`{}` needs a name"));
                    }
                    if params.contains(&name) {
                        return Err(invalid("a `{name}` is used twice"));
                    }
                    regex.push_str(&format!("(?P<{name}>[^@]+?)"));
                    params.push(name);
                }
                '}' => return Err(invalid("a `}` has no `{`")),
                '*' => {
                    wildcard = true;
                    if trimmed == "*" {
                        regex.push_str(".*");
                    } else {
                        regex.push_str("[^@]*");
                    }
                }
                other => regex.push_str(&regex::escape(&other.to_string())),
            }
        }
        regex.push('$');
        let regex = Regex::new(&regex).map_err(|error| invalid(&error.to_string()))?;
        let rank = if wildcard {
            2
        } else {
            u8::from(!params.is_empty())
        };
        Ok(Self {
            pattern: trimmed.to_owned(),
            regex,
            params,
            rank,
        })
    }

    /// The pattern as written.
    pub fn as_str(&self) -> &str {
        &self.pattern
    }

    /// The `{name}` captures, in order.
    pub fn params(&self) -> &[String] {
        &self.params
    }

    /// `address`'s captures when it matches.
    pub fn matches(&self, address: &str) -> Option<Vec<(String, String)>> {
        let captures = self.regex.captures(address.trim())?;
        Some(
            self.params
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        captures
                            .name(name)
                            .map(|m| m.as_str().to_owned())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        )
    }

    /// 0 for exact addresses, 1 with `{name}`, 2 with `*`.
    pub(crate) fn rank(&self) -> u8 {
        self.rank
    }
}

impl fmt::Debug for MailboxPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MailboxPattern")
            .field(&self.pattern)
            .finish()
    }
}

impl fmt::Display for MailboxPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.pattern)
    }
}

/// Handles the messages of a mailbox.
///
/// Closures `Fn(InboundMail, MailboxContext) -> impl Future<Output =
/// Result<(), BoxError>>` are handlers; a type that needs services
/// implements this trait and [`Injectable`] and is registered with
/// [`mailbox_with`](ModuleBuilderMailExt::mailbox_with).
///
/// - `Ok(())`: the message was handled.
/// - An error from [`MailboxContext::reject`], or a bad reply token from
///   [`MailboxContext::verified`]: the message is dropped for good (status
///   `rejected`); the provider is answered with a success.
/// - Any other error: the provider is answered with a 500 and delivers the
///   message again later, so a handler should be idempotent (the
///   [`fingerprint`](InboundMail::fingerprint) and
///   [`message_id`](InboundMail::message_id) identify a message).
pub trait MailboxHandler: Send + Sync + 'static {
    /// Handles one message.
    fn handle(&self, mail: InboundMail, ctx: MailboxContext)
    -> BoxFuture<'_, Result<(), BoxError>>;
}

impl<F, Fut> MailboxHandler for F
where
    F: Fn(InboundMail, MailboxContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
{
    fn handle(
        &self,
        mail: InboundMail,
        ctx: MailboxContext,
    ) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(self(mail, ctx))
    }
}

/// What a mailbox handler knows about the delivery: the pattern and the
/// address it matched, the `{name}` captures, the app and the reply tokens.
#[derive(Clone)]
pub struct MailboxContext {
    pattern: String,
    address: String,
    params: Vec<(String, String)>,
    app: AppContext,
    tokens: Arc<ReplyTokens>,
}

impl MailboxContext {
    pub(crate) fn new(
        pattern: String,
        address: String,
        params: Vec<(String, String)>,
        app: AppContext,
        tokens: Arc<ReplyTokens>,
    ) -> Self {
        Self {
            pattern,
            address,
            params,
            app,
            tokens,
        }
    }

    /// The mailbox pattern (`reply+{token}@in.acme.test`).
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The recipient address that matched, in lower case.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The `{name}` capture `name`, as written in the address (lower
    /// case).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(param, _)| param == name)
            .map(|(_, value)| value.as_str())
    }

    /// The value sealed in the reply token captured as `{name}`
    /// ([`ReplyTokens::verify`]).
    ///
    /// # Errors
    ///
    /// [`InboundError::InvalidToken`] (also when there is no such capture)
    /// or [`InboundError::ExpiredToken`]; returned from the handler with
    /// `?`, either rejects the message.
    pub fn verified(&self, name: &str) -> Result<String, InboundError> {
        let token = self.param(name).ok_or(InboundError::InvalidToken)?;
        self.tokens.verify(token)
    }

    /// The UUID sealed in the reply token captured as `{name}`
    /// ([`ReplyTokens::verify_uuid`]).
    ///
    /// # Errors
    ///
    /// As [`verified`](MailboxContext::verified).
    pub fn verified_uuid(&self, name: &str) -> Result<Uuid, InboundError> {
        let token = self.param(name).ok_or(InboundError::InvalidToken)?;
        self.tokens.verify_uuid(token)
    }

    /// The app's reply tokens.
    pub fn tokens(&self) -> &ReplyTokens {
        &self.tokens
    }

    /// The app.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// A provider from the app (`ctx.get::<Tickets>().await?`).
    ///
    /// # Errors
    ///
    /// The kernel's error when the app has no such provider.
    pub async fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>, KernelError> {
        self.app.get::<T>().await
    }

    /// An error that rejects the message for good: return it from the
    /// handler (`return Err(ctx.reject("unknown ticket"))`). The status is
    /// `rejected` and the provider is answered with a success.
    pub fn reject(&self, reason: impl Into<String>) -> BoxError {
        Box::new(InboundError::Rejected(reason.into()))
    }
}

impl fmt::Debug for MailboxContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailboxContext")
            .field("pattern", &self.pattern)
            .field("address", &self.address)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

/// Resolves an injectable handler from the module that registered it.
pub(crate) type ResolveHandler =
    fn(AppContext, ModuleId) -> BoxFuture<'static, Result<Arc<dyn MailboxHandler>, BoxError>>;

/// How a mailbox gets its handler.
#[derive(Clone)]
pub(crate) enum HandlerSource {
    /// A handler value (a closure).
    Value(Arc<dyn MailboxHandler>),
    /// A provider of the registering module.
    Provider(ResolveHandler),
}

/// One `m.mailbox(..)`, contributed by the registering module.
#[derive(Clone)]
pub(crate) struct MailboxRegistration {
    pub(crate) pattern: String,
    pub(crate) handler: HandlerSource,
}

/// Inbound mail registrations on a [`ModuleBuilder`] (crate feature
/// `inbound`).
///
/// ```no_run
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_mail::{InboundMail, MailboxContext, ModuleBuilderMailExt};
///
/// struct SupportModule;
///
/// impl Module for SupportModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.mailbox("reply+{token}@in.acme.test", |mail: InboundMail, ctx: MailboxContext| async move {
///             let ticket = ctx.verified_uuid("token")?;
///             println!("{ticket}: {}", mail.reply_text().unwrap_or_default());
///             Ok(())
///         });
///     }
/// }
/// ```
pub trait ModuleBuilderMailExt {
    /// Routes the messages for addresses matching `pattern` (see
    /// [`MailboxPattern`]) to `handler`. An invalid pattern fails the boot.
    fn mailbox<H: MailboxHandler>(&mut self, pattern: &str, handler: H) -> &mut Self;

    /// Registers `H` as a provider of this module and routes the messages
    /// for addresses matching `pattern` to it (resolved with its
    /// dependencies, in a request scope of its own per message).
    fn mailbox_with<H: MailboxHandler + Injectable>(&mut self, pattern: &str) -> &mut Self;
}

impl ModuleBuilderMailExt for ModuleBuilder {
    fn mailbox<H: MailboxHandler>(&mut self, pattern: &str, handler: H) -> &mut Self {
        self.contribute(MailboxRegistration {
            pattern: pattern.to_owned(),
            handler: HandlerSource::Value(Arc::new(handler)),
        })
    }

    fn mailbox_with<H: MailboxHandler + Injectable>(&mut self, pattern: &str) -> &mut Self {
        self.provider::<H>().contribute(MailboxRegistration {
            pattern: pattern.to_owned(),
            handler: HandlerSource::Provider(resolve::<H>),
        })
    }
}

/// Resolves `H` from `module` in a fresh request scope.
fn resolve<H: MailboxHandler + Injectable>(
    app: AppContext,
    module: ModuleId,
) -> BoxFuture<'static, Result<Arc<dyn MailboxHandler>, BoxError>> {
    Box::pin(async move {
        let handler = app
            .request_resolver(module, RequestScope::new())
            .get::<H>()
            .await?;
        Ok(handler as Arc<dyn MailboxHandler>)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pattern: &str, address: &str) -> Option<Vec<(String, String)>> {
        MailboxPattern::parse(pattern).unwrap().matches(address)
    }

    #[test]
    fn exact_addresses_match_without_case() {
        assert_eq!(
            params("support@acme.test", "Support@ACME.test"),
            Some(vec![])
        );
        assert_eq!(params("support@acme.test", "support@agra.testing"), None);
        assert_eq!(params("support@acme.test", "xsupport@acme.test"), None);
        assert_eq!(
            params("a.b@acme.test", "axb@acme.test"),
            None,
            "dots are literal"
        );
    }

    #[test]
    fn params_capture_part_of_the_address() {
        assert_eq!(
            params("reply+{token}@in.acme.test", "reply+k5q2@in.acme.test"),
            Some(vec![("token".into(), "k5q2".into())])
        );
        assert_eq!(
            params("reply+{token}@in.acme.test", "reply+@in.acme.test"),
            None
        );
        assert_eq!(
            params("{user}@{tenant}.in.acme.test", "anu@globex.in.acme.test"),
            Some(vec![
                ("user".into(), "anu".into()),
                ("tenant".into(), "gulati".into())
            ])
        );
        assert_eq!(
            params("{user}@in.acme.test", "a@b@in.acme.test"),
            None,
            "captures stop at the @"
        );
    }

    #[test]
    fn wildcards_match_runs_and_everything() {
        assert_eq!(
            params("*@in.acme.test", "anything@in.acme.test"),
            Some(vec![])
        );
        assert_eq!(params("*@in.acme.test", "x@acme.test"), None);
        assert_eq!(params("*", "who@ever.test"), Some(vec![]));
    }

    #[test]
    fn patterns_rank_exact_then_params_then_wildcards() {
        let rank = |p: &str| MailboxPattern::parse(p).unwrap().rank();
        assert_eq!(rank("support@acme.test"), 0);
        assert_eq!(rank("reply+{token}@acme.test"), 1);
        assert_eq!(rank("*@acme.test"), 2);
        assert_eq!(rank("*"), 2);
    }

    #[test]
    fn invalid_patterns_are_refused() {
        for pattern in [
            "",
            "support",
            "a@b@c",
            "reply+{token@acme.test",
            "reply+{}@acme.test",
            "reply+{a}{a}@acme.test",
            "reply+}@acme.test",
            "reply+{to-ken}@acme.test",
        ] {
            assert!(
                matches!(
                    MailboxPattern::parse(pattern),
                    Err(InboundError::InvalidPattern { .. })
                ),
                "{pattern}"
            );
        }
    }
}
