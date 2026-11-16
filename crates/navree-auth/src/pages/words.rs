//! [`AuthWords`]: what the sign-in pages say, in the app's own words.

/// The words of the sign-in pages: each page's title and the line under it, and the labels of
/// their fields. Each is the pages' own English until you give yours, in your app's voice or
/// its language; hand them to [`AuthPages::words`](super::AuthPages::words). They are text,
/// escaped when the page is drawn, so a word can't add markup.
///
/// A page's title is also its tab's title. A label renames the field wherever it is drawn: the
/// email address on the sign-in, sign-up, forgotten-password, sign-in link and sign-in code
/// pages takes [`email_label`](Self::email_label). The buttons, the links, the notices and the
/// errors keep their words.
///
/// ```
/// use navree_auth::{AuthPages, AuthWords};
///
/// let pages = AuthPages::new().words(
///     AuthWords::new()
///         .sign_in("Sign in to Agra CRM", "Your deals, your contacts and the week ahead.")
///         .email_label("Work email"),
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct AuthWords {
    pages: [Option<(String, String)>; PAGES],
    labels: [Option<String>; LABELS],
}

/// How many pages have words of their own.
const PAGES: usize = 14;

/// How many fields have a label of their own.
const LABELS: usize = 10;

/// A page the pages draw, for its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    SignIn,
    SignUp,
    TwoFactor,
    ForgotPassword,
    ResetPassword,
    MagicLink,
    MagicLinkConfirm,
    Code,
    CodeEntry,
    Passkey,
    TwoFactorSetup,
    TwoFactorSecret,
    RecoveryCodes,
    TwoFactorOn,
}

/// Each page's title and the line under it, by [`Page`]: the words the pages have always said.
const PAGE_WORDS: [(&str, &str); PAGES] = [
    (
        "Sign in",
        "Use the email address and password of your account.",
    ),
    (
        "Create an account",
        "Your name, your email address and a password.",
    ),
    (
        "Two-factor authentication",
        "Enter the code your authenticator app shows now, or one of your recovery codes.",
    ),
    (
        "Forgot your password?",
        "We will email you a link to choose a new one.",
    ),
    (
        "Choose a new password",
        "Changing it signs you out everywhere.",
    ),
    (
        "Email me a sign-in link",
        "We will email you a link that signs you in, without a password.",
    ),
    (
        "Continue signing in",
        "Press the button to finish signing in with the link from your email.",
    ),
    (
        "Email me a sign-in code",
        "We will email you a short code that signs you in, without a password.",
    ),
    (
        "Enter the code",
        "Type the code from the email we sent you.",
    ),
    (
        "Sign in with a passkey",
        "Use the passkey on this device, or one from your phone or security key.",
    ),
    (
        "Set up two-factor authentication",
        "Signing in will ask for a code from an authenticator app on your phone, as well as your password.",
    ),
    (
        "Set up two-factor authentication",
        "Scan the picture with an authenticator app, or type the key into it, then enter the code it shows.",
    ),
    (
        "Save your recovery codes",
        "Each code works once, if you lose your phone. They are shown only now.",
    ),
    (
        "Two-factor authentication",
        "It is on: signing in asks for a code from your authenticator app.",
    ),
];

/// A field the pages draw, for its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Label {
    Email,
    Password,
    Remember,
    Name,
    PasswordAgain,
    NewPassword,
    NewPasswordAgain,
    AuthenticatorCode,
    RecoveryCode,
    EmailedCode,
}

impl AuthWords {
    /// The pages' own words.
    pub fn new() -> Self {
        Self::default()
    }

    fn page(mut self, page: Page, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.pages[page as usize] = Some((title.into(), lead.into()));
        self
    }

    fn label(mut self, label: Label, text: impl Into<String>) -> Self {
        self.labels[label as usize] = Some(text.into());
        self
    }

    /// The sign-in page's title and the line under it ("Sign in"; "Use the email address and
    /// password of your account.").
    pub fn sign_in(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::SignIn, title, lead)
    }

    /// The sign-up page's ("Create an account"; "Your name, your email address and a
    /// password.").
    pub fn sign_up(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::SignUp, title, lead)
    }

    /// The page that asks for the second factor while signing in ("Two-factor
    /// authentication"; "Enter the code your authenticator app shows now, or one of your
    /// recovery codes.").
    pub fn two_factor(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::TwoFactor, title, lead)
    }

    /// The forgotten-password page's ("Forgot your password?"; "We will email you a link to
    /// choose a new one.").
    pub fn forgot_password(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::ForgotPassword, title, lead)
    }

    /// The page that sets a new password ("Choose a new password"; "Changing it signs you out
    /// everywhere.").
    pub fn reset_password(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::ResetPassword, title, lead)
    }

    /// The page that mails a sign-in link ("Email me a sign-in link"; "We will email you a
    /// link that signs you in, without a password.").
    pub fn magic_link(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::MagicLink, title, lead)
    }

    /// The page a mailed sign-in link opens, with its one button ("Continue signing in";
    /// "Press the button to finish signing in with the link from your email.").
    pub fn magic_link_confirm(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::MagicLinkConfirm, title, lead)
    }

    /// The page that mails a sign-in code ("Email me a sign-in code"; "We will email you a
    /// short code that signs you in, without a password.").
    pub fn code(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::Code, title, lead)
    }

    /// The page that asks for the mailed code ("Enter the code"; "Type the code from the email
    /// we sent you.").
    pub fn code_entry(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::CodeEntry, title, lead)
    }

    /// The passkey page's ("Sign in with a passkey"; "Use the passkey on this device, or one
    /// from your phone or security key.").
    pub fn passkey(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::Passkey, title, lead)
    }

    /// The set-up page of two-factor authentication while it is off ("Set up two-factor
    /// authentication"; "Signing in will ask for a code from an authenticator app on your
    /// phone, as well as your password.").
    pub fn two_factor_setup(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::TwoFactorSetup, title, lead)
    }

    /// The set-up page that shows the secret and asks for the first code ("Set up two-factor
    /// authentication"; "Scan the picture with an authenticator app, or type the key into it,
    /// then enter the code it shows.").
    pub fn two_factor_secret(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::TwoFactorSecret, title, lead)
    }

    /// The page that shows the recovery codes once ("Save your recovery codes"; "Each code
    /// works once, if you lose your phone. They are shown only now.").
    pub fn recovery_codes(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::RecoveryCodes, title, lead)
    }

    /// The set-up page once two-factor authentication is on ("Two-factor authentication";
    /// "It is on: signing in asks for a code from your authenticator app.").
    pub fn two_factor_on(self, title: impl Into<String>, lead: impl Into<String>) -> Self {
        self.page(Page::TwoFactorOn, title, lead)
    }

    /// The email address's label ("Email") on every page that asks for it.
    pub fn email_label(self, text: impl Into<String>) -> Self {
        self.label(Label::Email, text)
    }

    /// The password's label on the sign-in and sign-up pages ("Password").
    pub fn password_label(self, text: impl Into<String>) -> Self {
        self.label(Label::Password, text)
    }

    /// The label of the box that keeps a browser signed in ("Keep me signed in on this
    /// device"), on every page that has it.
    pub fn remember_label(self, text: impl Into<String>) -> Self {
        self.label(Label::Remember, text)
    }

    /// The name's label on the sign-up page ("Your name").
    pub fn name_label(self, text: impl Into<String>) -> Self {
        self.label(Label::Name, text)
    }

    /// The label of the password typed again on the sign-up page ("Password again").
    pub fn password_again_label(self, text: impl Into<String>) -> Self {
        self.label(Label::PasswordAgain, text)
    }

    /// The new password's label on the page that sets it ("New password").
    pub fn new_password_label(self, text: impl Into<String>) -> Self {
        self.label(Label::NewPassword, text)
    }

    /// The label of the new password typed again ("New password again").
    pub fn new_password_again_label(self, text: impl Into<String>) -> Self {
        self.label(Label::NewPasswordAgain, text)
    }

    /// The label of the code from an authenticator app, while signing in and while setting it
    /// up ("Code from your authenticator app").
    pub fn authenticator_code_label(self, text: impl Into<String>) -> Self {
        self.label(Label::AuthenticatorCode, text)
    }

    /// The recovery code's label on the second-factor page ("Or a recovery code").
    pub fn recovery_code_label(self, text: impl Into<String>) -> Self {
        self.label(Label::RecoveryCode, text)
    }

    /// The label of the code from the email ("Code from the email").
    pub fn emailed_code_label(self, text: impl Into<String>) -> Self {
        self.label(Label::EmailedCode, text)
    }

    /// `page`'s title: the app's, else the page's own.
    pub(super) fn title(&self, page: Page) -> &str {
        self.pages[page as usize]
            .as_ref()
            .map_or(PAGE_WORDS[page as usize].0, |(title, _)| title)
    }

    /// The line under `page`'s title: the app's, else the page's own.
    pub(super) fn lead(&self, page: Page) -> &str {
        self.pages[page as usize]
            .as_ref()
            .map_or(PAGE_WORDS[page as usize].1, |(_, lead)| lead)
    }

    /// The app's label for `label`, when it gave one: without it the field keeps the label
    /// its form gives it.
    pub(super) fn label_of(&self, label: Label) -> Option<&str> {
        self.labels[label as usize].as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A setter of a page's title and line.
    type PageSetter = fn(AuthWords, String, String) -> AuthWords;

    /// A setter of a field's label.
    type LabelSetter = fn(AuthWords, String) -> AuthWords;

    #[test]
    fn a_page_says_its_own_words_until_the_app_gives_others() {
        let own = AuthWords::new();
        assert_eq!(own.title(Page::SignIn), "Sign in");
        assert_eq!(
            own.lead(Page::SignIn),
            "Use the email address and password of your account."
        );
        assert_eq!(own.label_of(Label::Email), None);
        let words = AuthWords::new()
            .sign_in("Sign in to Agra CRM", "Your deals and the week ahead.")
            .email_label("Work email");
        assert_eq!(words.title(Page::SignIn), "Sign in to Agra CRM");
        assert_eq!(words.lead(Page::SignIn), "Your deals and the week ahead.");
        assert_eq!(words.label_of(Label::Email), Some("Work email"));
        // The other pages keep theirs.
        assert_eq!(words.title(Page::SignUp), "Create an account");
        assert_eq!(words.label_of(Label::Password), None);
    }

    #[test]
    fn every_page_and_every_label_has_a_setter_of_its_own() {
        let pages: [(PageSetter, Page); PAGES] = [
            (|w, t, l| w.sign_in(t, l), Page::SignIn),
            (|w, t, l| w.sign_up(t, l), Page::SignUp),
            (|w, t, l| w.two_factor(t, l), Page::TwoFactor),
            (|w, t, l| w.forgot_password(t, l), Page::ForgotPassword),
            (|w, t, l| w.reset_password(t, l), Page::ResetPassword),
            (|w, t, l| w.magic_link(t, l), Page::MagicLink),
            (|w, t, l| w.magic_link_confirm(t, l), Page::MagicLinkConfirm),
            (|w, t, l| w.code(t, l), Page::Code),
            (|w, t, l| w.code_entry(t, l), Page::CodeEntry),
            (|w, t, l| w.passkey(t, l), Page::Passkey),
            (|w, t, l| w.two_factor_setup(t, l), Page::TwoFactorSetup),
            (|w, t, l| w.two_factor_secret(t, l), Page::TwoFactorSecret),
            (|w, t, l| w.recovery_codes(t, l), Page::RecoveryCodes),
            (|w, t, l| w.two_factor_on(t, l), Page::TwoFactorOn),
        ];
        for (at, (set, page)) in pages.into_iter().enumerate() {
            assert_eq!(page as usize, at);
            let words = set(
                AuthWords::new(),
                format!("title {at}"),
                format!("lead {at}"),
            );
            assert_eq!(words.title(page), format!("title {at}"));
            assert_eq!(words.lead(page), format!("lead {at}"));
            assert_eq!(words.pages.iter().flatten().count(), 1, "{page:?} alone");
        }
        let labels: [(LabelSetter, Label); LABELS] = [
            (|w, t| w.email_label(t), Label::Email),
            (|w, t| w.password_label(t), Label::Password),
            (|w, t| w.remember_label(t), Label::Remember),
            (|w, t| w.name_label(t), Label::Name),
            (|w, t| w.password_again_label(t), Label::PasswordAgain),
            (|w, t| w.new_password_label(t), Label::NewPassword),
            (
                |w, t| w.new_password_again_label(t),
                Label::NewPasswordAgain,
            ),
            (
                |w, t| w.authenticator_code_label(t),
                Label::AuthenticatorCode,
            ),
            (|w, t| w.recovery_code_label(t), Label::RecoveryCode),
            (|w, t| w.emailed_code_label(t), Label::EmailedCode),
        ];
        for (at, (set, label)) in labels.into_iter().enumerate() {
            assert_eq!(label as usize, at);
            let words = set(AuthWords::new(), format!("label {at}"));
            assert_eq!(words.label_of(label), Some(format!("label {at}").as_str()));
            assert_eq!(words.labels.iter().flatten().count(), 1, "{label:?} alone");
        }
    }
}
