//! [`SocialFake`]: decides what each code returns, for tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::{ProviderToken, SocialError, SocialUser};

/// What a faked code answers.
enum Answer {
    User(Box<SocialUser>),
    Error(String),
}

/// The fake's shared state.
#[derive(Default)]
pub(crate) struct FakeState {
    codes: Mutex<HashMap<(String, String), Answer>>,
    refreshes: Mutex<HashMap<String, ProviderToken>>,
    exchanged: Mutex<Vec<(String, String)>>,
}

impl FakeState {
    /// The code exchange and profile request of `provider`: the user or
    /// error set for `code`, or the `invalid_grant` refusal a provider
    /// gives an unknown code.
    pub(crate) fn exchange(
        &self,
        provider: &str,
        code: &str,
        provider_name: &str,
    ) -> Result<SocialUser, SocialError> {
        self.exchanged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((provider.to_owned(), code.to_owned()));
        let codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        match codes.get(&(provider.to_owned(), code.to_owned())) {
            Some(Answer::User(user)) => {
                let mut user = (**user).clone();
                user.provider = provider.to_owned();
                Ok(user)
            }
            Some(Answer::Error(error)) => Err(SocialError::provider(provider_name, error.clone())),
            None => Err(SocialError::provider(provider_name, "invalid_grant")),
        }
    }

    /// A token refresh of `provider`: the token set with
    /// [`SocialFake::on_refresh`], or [`SocialError::Reauthorize`].
    pub(crate) fn refresh(
        &self,
        provider: &str,
        provider_name: &str,
    ) -> Result<ProviderToken, SocialError> {
        self.refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(provider)
            .cloned()
            .ok_or_else(|| SocialError::Reauthorize {
                provider: provider_name.to_owned(),
            })
    }
}

/// The other half of [`Social::fake`](crate::Social::fake): decides what
/// each code returns. The fake `Social` never calls a provider, but seals
/// and checks states, links accounts and issues tokens like the real one.
///
/// ```
/// use navree_social::{Social, SocialUser};
///
/// let (_social, fake) = Social::fake();
/// fake.on_code(
///     "github",
///     "code-ada",
///     SocialUser::new("github", "583231").email(Some("ada@example.com"), true),
/// );
/// fake.on_code_error("github", "code-denied", "access_denied");
/// assert!(fake.exchanged().is_empty());
/// ```
#[derive(Clone)]
pub struct SocialFake {
    state: Arc<FakeState>,
}

impl SocialFake {
    pub(crate) fn new(state: Arc<FakeState>) -> Self {
        Self { state }
    }

    /// Answers `code` from `provider` with `user` (its `provider` is set to
    /// `provider`).
    pub fn on_code(&self, provider: &str, code: &str, user: SocialUser) -> &Self {
        self.state
            .codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                (provider.to_owned(), code.to_owned()),
                Answer::User(Box::new(user)),
            );
        self
    }

    /// Answers `code` from `provider` with a provider error (an OAuth error
    /// code such as `access_denied` or `invalid_grant`): the callback is a
    /// 400 "<Provider> refused the sign-in: <error>".
    pub fn on_code_error(&self, provider: &str, code: &str, error: impl Into<String>) -> &Self {
        self.state
            .codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                (provider.to_owned(), code.to_owned()),
                Answer::Error(error.into()),
            );
        self
    }

    /// Answers token refreshes of `provider` ([`Social::token`](crate::Social::token))
    /// with `token`; without it, a refresh is refused and `token` returns
    /// [`SocialError::Reauthorize`].
    pub fn on_refresh(&self, provider: &str, token: ProviderToken) -> &Self {
        self.state
            .refreshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(provider.to_owned(), token);
        self
    }

    /// The `(provider, code)` pairs exchanged so far, in order (only codes
    /// whose state passed the checks).
    pub fn exchanged(&self) -> Vec<(String, String)> {
        self.state
            .exchanged
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl std::fmt::Debug for SocialFake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocialFake").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_answer_users_errors_or_invalid_grant() {
        let state = Arc::new(FakeState::default());
        let fake = SocialFake::new(state.clone());
        fake.on_code("github", "ok", SocialUser::new("other", "1"));
        fake.on_code_error("github", "denied", "access_denied");
        let user = state.exchange("github", "ok", "GitHub").unwrap();
        assert_eq!(user.provider, "github");
        let denied = state.exchange("github", "denied", "GitHub").unwrap_err();
        assert_eq!(
            denied.to_string(),
            "GitHub refused the sign-in: access_denied"
        );
        let unknown = state.exchange("github", "nope", "GitHub").unwrap_err();
        assert!(unknown.to_string().contains("invalid_grant"));
        assert_eq!(fake.exchanged().len(), 3);
        assert_eq!(fake.exchanged()[0], ("github".to_owned(), "ok".to_owned()));
    }

    #[test]
    fn refreshes_need_an_answer() {
        let state = Arc::new(FakeState::default());
        let fake = SocialFake::new(state.clone());
        assert!(matches!(
            state.refresh("google", "Google"),
            Err(SocialError::Reauthorize { .. })
        ));
        fake.on_refresh("google", ProviderToken::new("ya29.new"));
        assert_eq!(
            state
                .refresh("google", "Google")
                .unwrap()
                .access_token
                .expose(),
            "ya29.new"
        );
    }
}
