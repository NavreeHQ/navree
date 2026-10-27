//! The digest mail: several notifications' mails batched into one
//! ([`DigestItem`], and the mailable that renders them).

use chrono::{DateTime, Utc};
use navree_mail::{MailError, Mailable, Message, TemplateMail};
use navree_view::Views;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::overrides::{StyledMail, with_style};
use crate::{Digest, DigestOptions, NotificationOverrides};

/// The notification type of digest mails: the key of their
/// [`MailOverride`](crate::MailOverride) in a workspace's
/// [`NotificationOverrides`], and the type their telemetry reports.
pub const DIGEST_TYPE: &str = "notify.digest";

/// The built-in digest templates (`notify.digest.template`); files of the
/// same names replace them.
pub(crate) const DIGEST_TEMPLATES: [(&str, &str); 2] = [
    (
        "notify/digest.html",
        r#"<!doctype html>
<html lang="{% if locale %}{{ locale }}{% else %}en{% endif %}">
<body style="margin: 0; padding: 24px; font-family: -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; color: #111827">
{% if style.logo_url %}<img src="{{ style.logo_url }}" alt="" style="max-height: 40px; margin-bottom: 16px">{% endif %}
<h1 style="font-size: 18px; margin: 0 0 16px">{{ count }} notification{% if count != 1 %}s{% endif %}</h1>
{% for item in items %}
<section style="border-top: 1px solid #e5e7eb; padding: 12px 0">
<h2 style="font-size: 15px; margin: 0 0 4px{% if style.brand_color %}; color: {{ style.brand_color }}{% endif %}">{{ item.subject }}</h2>
{% if item.text %}<p style="margin: 0; white-space: pre-line; color: #374151">{{ item.text }}</p>{% endif %}
</section>
{% endfor %}
{% if style.footer %}<p style="margin-top: 24px; font-size: 12px; color: #6b7280">{{ style.footer }}</p>{% endif %}
</body>
</html>
"#,
    ),
    (
        "notify/digest.txt",
        "{{ count }} notification{% if count != 1 %}s{% endif %}\n\
         {% for item in items %}\n- {{ item.subject }}{% if item.text %}\n  {{ item.text }}{% endif %}\n{% endfor %}\
         {% if style.footer %}\n{{ style.footer }}\n{% endif %}",
    ),
];

/// One notification's mail held for a digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DigestItem {
    /// The notification id.
    pub id: Uuid,
    /// [`Notification::TYPE`](crate::Notification::TYPE).
    #[serde(rename = "type")]
    pub notification_type: String,
    /// [`Notification::CATEGORY`](crate::Notification::CATEGORY).
    pub category: Option<String>,
    /// The mail it would have sent.
    pub mail: TemplateMail,
    /// When it was sent.
    pub created_at: DateTime<Utc>,
}

/// The digest of `items`, rendered with `notify.digest.template`.
pub(crate) struct DigestMail<'a> {
    pub(crate) to: &'a [String],
    pub(crate) digest: Digest,
    pub(crate) items: &'a [DigestItem],
    pub(crate) options: &'a DigestOptions,
    pub(crate) overrides: Option<&'a NotificationOverrides>,
}

impl DigestMail<'_> {
    /// The digest template's context: `items` (each with `type`,
    /// `category`, `subject`, `created_at`, `context` and its rendered
    /// `html` and `text`), `count`, `digest` and `style`.
    fn context(&self, views: &Views) -> Value {
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|item| {
                let rendered = StyledMail {
                    mail: &item.mail,
                    notification_type: &item.notification_type,
                    overrides: self.overrides,
                }
                .build(views);
                let (subject, html, text) = match rendered {
                    Ok(message) => (message.subject, message.html, message.text),
                    Err(error) => {
                        tracing::warn!(
                            target: "navree::notify",
                            notification_type = %item.notification_type,
                            %error,
                            "a digest item's mail failed to render; listing its subject only"
                        );
                        (item.mail.subject.clone(), None, None)
                    }
                };
                json!({
                    "id": item.id,
                    "type": item.notification_type,
                    "category": item.category,
                    "subject": subject,
                    "created_at": item.created_at,
                    "context": item.mail.context,
                    "html": html,
                    "text": text,
                })
            })
            .collect();
        let locale = self
            .items
            .iter()
            .rev()
            .find_map(|item| item.mail.context.get("locale").cloned());
        let mut context = json!({
            "items": items,
            "count": self.items.len(),
            "digest": self.digest,
        });
        if let (Some(locale), Some(fields)) = (locale, context.as_object_mut()) {
            fields.insert("locale".to_owned(), locale);
        }
        with_style(&context, self.overrides)
    }
}

impl Mailable for DigestMail<'_> {
    fn build(&self, views: &Views) -> Result<Message, MailError> {
        let context = self.context(views);
        let template = self
            .overrides
            .and_then(|overrides| overrides.templates.get(DIGEST_TYPE));
        let subject_source = template
            .and_then(|template| template.subject.as_deref())
            .unwrap_or(&self.options.subject);
        let subject = match views.render_source("notify.digest.txt", subject_source, &context) {
            Ok(subject) => subject.trim().to_owned(),
            Err(error) => {
                tracing::warn!(
                    target: "navree::notify",
                    %error,
                    "the digest subject failed to render; sending it as written"
                );
                subject_source.to_owned()
            }
        };
        let mail = TemplateMail {
            to: self.to.to_vec(),
            subject: subject.clone(),
            template: self.options.template.clone(),
            context,
        };
        let mut message = StyledMail {
            mail: &mail,
            notification_type: DIGEST_TYPE,
            overrides: self.overrides,
        }
        .build(views)?;
        // The subject override is already applied.
        message.subject = subject;
        Ok(message)
    }

    fn template(&self) -> Option<&str> {
        Some(&self.options.template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MailOverride;

    fn views() -> Views {
        let mut templates: Vec<(String, String)> = DIGEST_TEMPLATES
            .iter()
            .map(|(name, source)| ((*name).to_owned(), (*source).to_owned()))
            .collect();
        templates.push((
            "mail/deal-won.txt".to_owned(),
            "{{ title }} was won.".to_owned(),
        ));
        Views::from_templates(templates).unwrap()
    }

    fn item(title: &str) -> DigestItem {
        DigestItem {
            id: Uuid::now_v7(),
            notification_type: "deal_won".to_owned(),
            category: Some("deals".to_owned()),
            mail: TemplateMail {
                to: vec!["ada@example.com".to_owned()],
                subject: format!("Deal won: {title}"),
                template: "mail/deal-won".to_owned(),
                context: json!({"title": title}),
            },
            created_at: "2026-10-29T08:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn one_mail_lists_every_item() {
        let items = [item("Acme"), item("Globex")];
        let options = DigestOptions::default();
        let to = ["ada@example.com".to_owned()];
        let message = DigestMail {
            to: &to,
            digest: Digest::Daily,
            items: &items,
            options: &options,
            overrides: None,
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.to, ["ada@example.com"]);
        assert_eq!(message.subject, "Your notifications");
        let text = message.text.unwrap();
        assert!(text.starts_with("2 notifications"), "{text}");
        assert!(text.contains("- Deal won: Acme\n  Acme was won."), "{text}");
        assert!(
            text.contains("- Deal won: Globex\n  Globex was won."),
            "{text}"
        );
        let html = message.html.unwrap();
        assert!(
            html.contains("<h2 style=\"font-size: 15px; margin: 0 0 4px\">Deal won: Acme</h2>"),
            "{html}"
        );
    }

    #[test]
    fn a_workspace_restyles_the_digest() {
        let items = [item("Acme")];
        let options = DigestOptions {
            subject: "{{ count }} update{% if count != 1 %}s{% endif %} ({{ digest }})".to_owned(),
            ..DigestOptions::default()
        };
        let mut overrides = NotificationOverrides::default();
        overrides
            .style
            .insert("footer".to_owned(), json!("Acme Corp, Pune"));
        overrides
            .style
            .insert("brand_color".to_owned(), json!("#0F766E"));
        let to = ["ada@example.com".to_owned()];
        let message = DigestMail {
            to: &to,
            digest: Digest::Weekly,
            items: &items,
            options: &options,
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.subject, "1 update (weekly)");
        assert!(message.text.as_deref().unwrap().contains("Acme Corp, Pune"));
        assert!(message.html.as_deref().unwrap().contains("color: #0F766E"));

        overrides.templates.insert(
            DIGEST_TYPE.to_owned(),
            MailOverride {
                subject: Some("Acme digest".to_owned()),
                text: Some("{% for item in items %}* {{ item.subject }}{% endfor %}".to_owned()),
                html: None,
            },
        );
        let message = DigestMail {
            to: &to,
            digest: Digest::Weekly,
            items: &items,
            options: &options,
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.subject, "Acme digest");
        assert_eq!(message.text.as_deref(), Some("* Deal won: Acme"));
    }
}
