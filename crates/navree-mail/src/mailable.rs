//! [`Mailable`] and [`TemplateMail`].

use navree_view::Views;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{MailError, Message};

/// Something that renders into a [`Message`] (a mailable): send it
/// with [`Mailer::send`](crate::Mailer::send) or
/// [`Mailer::queue`](crate::Mailer::queue).
///
/// [`TemplateMail`] covers the common case; implement the trait for your own
/// types to compute the subject or recipients, or to use several templates:
///
/// ```
/// use navree_mail::{MailError, Mailable, Message};
/// use navree_view::Views;
/// use serde_json::json;
///
/// struct InvoicePaid {
///     email: String,
///     number: u32,
/// }
///
/// impl Mailable for InvoicePaid {
///     fn build(&self, views: &Views) -> Result<Message, MailError> {
///         let ctx = json!({ "number": self.number });
///         Ok(Message::new()
///             .to(&self.email)
///             .subject(format!("Invoice #{} paid", self.number))
///             .html(views.render("mail/invoice-paid.html", &ctx)?)
///             .text(views.render("mail/invoice-paid.txt", &ctx)?))
///     }
/// }
/// ```
pub trait Mailable: Send + Sync {
    /// Renders the message. Leave `from` unset to use the mailer's default
    /// sender.
    fn build(&self, views: &Views) -> Result<Message, MailError>;
}

/// A message is already built.
impl Mailable for Message {
    fn build(&self, _views: &Views) -> Result<Message, MailError> {
        Ok(self.clone())
    }
}

/// A mailable rendered from a template pair: `template` names the templates
/// without their extension, and `{template}.html` becomes the HTML body and
/// `{template}.txt` the plain-text body, whichever exist (at least one must).
/// A `template` with an extension renders just that template, as the
/// plain-text body for `.txt` and as the HTML body otherwise.
///
/// ```
/// use navree_mail::{Mailable, TemplateMail};
/// use navree_view::Views;
/// use serde_json::json;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let views = Views::from_templates([
///     ("mail/welcome.html", "<p>Hi {{ name }}</p>"),
///     ("mail/welcome.txt", "Hi {{ name }}"),
/// ])?;
/// let message = TemplateMail::new("ada@example.com", "Welcome", "mail/welcome")
///     .context(json!({ "name": "Ada" }))
///     .build(&views)?;
/// assert_eq!(message.html.as_deref(), Some("<p>Hi Ada</p>"));
/// assert_eq!(message.text.as_deref(), Some("Hi Ada"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TemplateMail {
    /// The recipients.
    pub to: Vec<String>,
    /// The subject line.
    pub subject: String,
    /// The template name, usually without extension (`mail/welcome`).
    pub template: String,
    /// The template variables (every printed variable must be defined).
    pub context: Value,
}

impl TemplateMail {
    /// A mail to `to` with an empty context.
    pub fn new(
        to: impl Into<String>,
        subject: impl Into<String>,
        template: impl Into<String>,
    ) -> Self {
        Self {
            to: vec![to.into()],
            subject: subject.into(),
            template: template.into(),
            context: Value::Object(serde_json::Map::new()),
        }
    }

    /// Sets the template variables.
    pub fn context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }
}

impl Mailable for TemplateMail {
    fn build(&self, views: &Views) -> Result<Message, MailError> {
        let render = |name: &str| views.render(name, &self.context);
        let html_name = format!("{}.html", self.template);
        let text_name = format!("{}.txt", self.template);
        let mut html = views
            .exists(&html_name)
            .then(|| render(&html_name))
            .transpose()?;
        let mut text = views
            .exists(&text_name)
            .then(|| render(&text_name))
            .transpose()?;
        if html.is_none() && text.is_none() {
            // A name with an extension, or no pair at all (reported as NotFound).
            let body = render(&self.template)?;
            if self.template.ends_with(".txt") {
                text = Some(body);
            } else {
                html = Some(body);
            }
        }
        Ok(Message {
            to: self.to.clone(),
            subject: self.subject.clone(),
            html,
            text,
            ..Message::default()
        })
    }
}
