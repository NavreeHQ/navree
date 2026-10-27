//! Rendering helpers the [`Mailer`](crate::Mailer) applies to every
//! message: CSS inlining, a plain-text body generated from the HTML, and
//! `lang`/`dir` from the message's locale ([`RenderOptions`], section
//! `mail.render`).
//!
//! ```
//! use navree_mail::render;
//!
//! let html = "<style>p { color: #333 }</style><p>Hi <a href=\"https://acme.test\">Ada</a></p>";
//! let inlined = render::inline_css(html).unwrap();
//! assert!(inlined.contains(r#"<p style="color: #333;">"#));
//!
//! let text = render::html_to_text("<p>Hi <b>Ada</b></p><p>Bye</p>");
//! assert_eq!(text, "Hi Ada\n\nBye");
//!
//! assert!(render::is_rtl("ar-EG"));
//! assert_eq!(
//!     render::apply_locale("<p>مرحبا</p>", "ar"),
//!     r#"<div lang="ar" dir="rtl"><p>مرحبا</p></div>"#
//! );
//! ```

use serde::Deserialize;

use crate::{MailError, Message};

/// How the mailer post-processes rendered mail. Configuration section
/// `mail.render`:
///
/// ```toml
/// [mail.render]
/// inline_css = true       # APP_MAIL__RENDER__INLINE_CSS
/// text_from_html = true   # APP_MAIL__RENDER__TEXT_FROM_HTML
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    /// Move the rules of `<style>` blocks into `style` attributes, which
    /// mail clients keep (default `true`). `@media` rules stay in a
    /// `<style>` block. HTML without a `<style>` block is left as it is.
    pub inline_css: bool,
    /// Generate the plain-text body from the HTML when a message has HTML
    /// only (default `true`), so it goes out as `multipart/alternative`.
    pub text_from_html: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            inline_css: true,
            text_from_html: true,
        }
    }
}

/// The width plain-text bodies are wrapped to.
const TEXT_WIDTH: usize = 78;

/// Applies `options` and the message's locale to `message`.
pub(crate) fn prepare(message: &mut Message, options: &RenderOptions) -> Result<(), MailError> {
    if let Some(html) = message.html.take() {
        let mut html = if options.inline_css && has_style(&html) {
            inline_css(&html)?
        } else {
            html
        };
        if let Some(locale) = message.locale.as_deref().filter(|l| !l.is_empty()) {
            html = apply_locale(&html, locale);
        }
        if options.text_from_html && message.text.is_none() {
            message.text = Some(html_to_text(&html));
        }
        message.html = Some(html);
    }
    Ok(())
}

/// Whether `html` has a `<style>` block.
fn has_style(html: &str) -> bool {
    html.to_ascii_lowercase().contains("<style")
}

/// `html` with the rules of its `<style>` blocks moved into `style`
/// attributes. `@media` and other at-rules stay in a `<style>` block;
/// remote stylesheets are never fetched. A fragment (no `<html>` element)
/// comes back as a whole document.
pub fn inline_css(html: &str) -> Result<String, MailError> {
    let inliner = css_inline::CSSInliner::options()
        .keep_style_tags(false)
        .keep_link_tags(true)
        .keep_at_rules(true)
        .load_remote_stylesheets(false)
        .build();
    inliner
        .inline(html)
        .map_err(|error| MailError::Config(format!("cannot inline the mail's CSS: {error}")))
}

/// A plain-text version of `html`: paragraphs and line breaks kept, lists
/// as `*` items, links as numbered footnotes (`[Open the deal][1]` ...
/// `[1]: https://...`), wrapped at 78 columns, without trailing blank
/// lines.
pub fn html_to_text(html: &str) -> String {
    let text = html2text::config::with_decorator(html2text::render::PlainDecorator::new())
        .link_footnotes(true)
        .string_from_read(html.as_bytes(), TEXT_WIDTH)
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim().to_owned()
}

/// The primary languages written right to left.
const RTL_LANGUAGES: &[&str] = &[
    "ar", "arc", "ckb", "dv", "fa", "he", "iw", "ks", "ku", "ps", "sd", "syr", "ug", "ur", "yi",
];

/// Whether `locale` (`ar`, `ar-EG`, `fa_IR`) is written right to left.
pub fn is_rtl(locale: &str) -> bool {
    let language = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    RTL_LANGUAGES.contains(&language.as_str())
}

/// `html` marked with `locale`: the `<html>` element gets `lang` and `dir`
/// (unless it has them), and a fragment is wrapped in
/// `<div lang=".." dir="..">`. `dir` is `rtl` for right-to-left languages
/// and `ltr` otherwise.
pub fn apply_locale(html: &str, locale: &str) -> String {
    let lang: String = locale
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .replace('_', "-");
    if lang.is_empty() {
        return html.to_owned();
    }
    let dir = if is_rtl(&lang) { "rtl" } else { "ltr" };
    let lower = html.to_ascii_lowercase();
    if let Some(start) = lower.find("<html") {
        let after = start + "<html".len();
        let next = lower[after..].chars().next();
        if matches!(next, Some(c) if c == '>' || c.is_whitespace()) {
            let end = lower[after..].find('>').map_or(html.len(), |i| after + i);
            let tag = &lower[after..end];
            let mut attributes = String::new();
            if !has_attribute(tag, "lang") {
                attributes.push_str(&format!(" lang=\"{lang}\""));
            }
            if !has_attribute(tag, "dir") {
                attributes.push_str(&format!(" dir=\"{dir}\""));
            }
            let mut out = String::with_capacity(html.len() + attributes.len());
            out.push_str(&html[..after]);
            out.push_str(&attributes);
            out.push_str(&html[after..]);
            return out;
        }
    }
    format!("<div lang=\"{lang}\" dir=\"{dir}\">{html}</div>")
}

/// Whether the (lower-case) attribute text of a tag has `name=`.
fn has_attribute(tag: &str, name: &str) -> bool {
    tag.split(|c: char| c.is_whitespace())
        .any(|part| part == name || part.starts_with(&format!("{name}=")))
}
