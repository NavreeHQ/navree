//! The account mails: the password reset link and the email verification
//! link, as built-in text or from the app's navree-view templates.

use navree_mail::{Mailable, Message, TemplateMail};
use serde_json::json;

use crate::{PasswordResetOptions, User, VerificationOptions};

/// The password reset mail for `user` carrying `token` (and a link to
/// `options.url` when configured).
pub(crate) fn password_reset(
    user: &User,
    token: &str,
    options: &PasswordResetOptions,
) -> Box<dyn Mailable> {
    let minutes = minutes(options.ttl_secs);
    let url = options
        .url
        .as_deref()
        .map(|url| with_query(url, &[("token", token), ("email", user.email.as_str())]));
    let subject = "Reset your password";
    if let Some(template) = &options.mail_template {
        return Box::new(
            TemplateMail::new(user.email.clone(), subject, template.clone()).context(json!({
                "name": user.name,
                "email": user.email,
                "token": token,
                "url": url.clone().unwrap_or_default(),
                "expires_in_minutes": minutes,
            })),
        );
    }
    let intro = "You are receiving this email because we received a password reset request \
                 for your account.";
    let outro = "If you did not request a password reset, no further action is required.";
    let expiry = format!("This password reset token expires in {minutes} minutes.");
    let (text_action, html_action) = match &url {
        Some(url) => (
            format!("Reset your password: {url}"),
            format!(
                r#"<p><a href="{href}">Reset your password</a></p>"#,
                href = escape_html(url)
            ),
        ),
        None => (
            format!("Your password reset token: {token}"),
            format!(
                "<p>Your password reset token: <code>{}</code></p>",
                escape_html(token)
            ),
        ),
    };
    Box::new(
        Message::new()
            .to(user.email.clone())
            .subject(subject)
            .text(format!(
                "Hello {name},\n\n{intro}\n\n{text_action}\n\n{expiry}\n\n{outro}\n",
                name = user.name
            ))
            .html(format!(
                "<p>Hello {name},</p>\n<p>{intro}</p>\n{html_action}\n<p>{expiry}</p>\n<p>{outro}</p>\n",
                name = escape_html(&user.name)
            )),
    )
}

/// The verification mail for `user` linking to `url`.
pub(crate) fn verification(
    user: &User,
    url: &str,
    options: &VerificationOptions,
) -> Box<dyn Mailable> {
    let minutes = minutes(options.ttl_secs);
    let subject = "Verify your email address";
    if let Some(template) = &options.mail_template {
        return Box::new(
            TemplateMail::new(user.email.clone(), subject, template.clone()).context(json!({
                "name": user.name,
                "email": user.email,
                "url": url,
                "expires_in_minutes": minutes,
            })),
        );
    }
    let outro = "If you did not create an account, no further action is required.";
    Box::new(
        Message::new()
            .to(user.email.clone())
            .subject(subject)
            .text(format!(
                "Hello {name},\n\nPlease confirm your email address: {url}\n\n\
                 This link expires in {minutes} minutes.\n\n{outro}\n",
                name = user.name
            ))
            .html(format!(
                "<p>Hello {name},</p>\n<p><a href=\"{href}\">Verify your email address</a></p>\n\
                 <p>This link expires in {minutes} minutes.</p>\n<p>{outro}</p>\n",
                name = escape_html(&user.name),
                href = escape_html(url)
            )),
    )
}

/// `url` with `pairs` appended to its query string (URL-encoded).
pub(crate) fn with_query(url: &str, pairs: &[(&str, &str)]) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{query}")
}

/// Whole minutes, rounded up.
fn minutes(secs: u64) -> u64 {
    secs.div_ceil(60)
}

/// `text` with the HTML special characters escaped.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_pairs_are_encoded_and_appended() {
        assert_eq!(
            with_query(
                "https://app.test/reset",
                &[("token", "a b"), ("email", "ada+x@example.com")]
            ),
            "https://app.test/reset?token=a+b&email=ada%2Bx%40example.com"
        );
        assert_eq!(
            with_query("https://app.test/reset?lang=en", &[("token", "t")]),
            "https://app.test/reset?lang=en&token=t"
        );
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            escape_html(r#"<b>"Ada" & 'Grace'</b>"#),
            "&lt;b&gt;&quot;Ada&quot; &amp; &#39;Grace&#39;&lt;/b&gt;"
        );
    }

    #[test]
    fn minutes_round_up() {
        assert_eq!(minutes(3600), 60);
        assert_eq!(minutes(61), 2);
    }
}
