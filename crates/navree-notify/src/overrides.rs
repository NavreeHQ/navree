//! Per-workspace notification mail: [`TemplateOverrides`] (where a
//! workspace's overrides come from), [`NotificationOverrides`] and the
//! mailables that apply them.

use std::collections::BTreeMap;

use navree_contracts::BoxFuture;
use navree_kernel::BoxError;
use navree_mail::{MailError, Mailable, Message, TemplateMail};
use navree_view::Views;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// Where a workspace's notification mail overrides come from: its style
/// (logo, brand color, footer) and its own subject and body per
/// notification type. The [`Notifier`](crate::Notifier) asks for them each
/// time it renders a notification's mail (or a digest), with the
/// notification's workspace ([`Notifiable::workspace_id`](crate::Notifiable::workspace_id)),
/// or `None` for mail sent outside any workspace.
///
/// `SettingsModule` registers one (`Arc<dyn TemplateOverrides>`) when
/// navree-settings is built with its `notify` feature: the overrides are
/// the `notification-templates` settings of the workspace, inherited from
/// its organization and the global values. Register your own to keep them
/// elsewhere.
///
/// ```
/// use navree_contracts::BoxFuture;
/// use navree_kernel::BoxError;
/// use navree_notify::{MailOverride, NotificationOverrides, TemplateOverrides};
/// use serde_json::json;
/// use uuid::Uuid;
///
/// struct Branded;
///
/// impl TemplateOverrides for Branded {
///     fn overrides(
///         &self,
///         _workspace: Option<Uuid>,
///     ) -> BoxFuture<'_, Result<NotificationOverrides, BoxError>> {
///         Box::pin(async {
///             let mut overrides = NotificationOverrides::default();
///             overrides.style.insert("brand_color".into(), json!("#0F766E"));
///             overrides.templates.insert(
///                 "deal_won".into(),
///                 MailOverride { subject: Some("We won {{ title }}!".into()), ..MailOverride::default() },
///             );
///             Ok(overrides)
///         })
///     }
/// }
/// ```
pub trait TemplateOverrides: Send + Sync + 'static {
    /// The overrides of `workspace`, or the app-wide ones for `None`
    /// (empty when there are none).
    fn overrides(
        &self,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'_, Result<NotificationOverrides, BoxError>>;
}

/// A workspace's notification mail overrides.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationOverrides {
    /// Variables every notification mail of the workspace gets as `style`
    /// (`{{ style.brand_color }}`, `{{ style.logo_url }}`), unless its
    /// context has a `style` of its own.
    pub style: Map<String, Value>,
    /// Subject and body overrides by notification type (`deal_won`; the
    /// digest mail is [`DIGEST_TYPE`](crate::DIGEST_TYPE)).
    pub templates: BTreeMap<String, MailOverride>,
}

impl NotificationOverrides {
    /// Whether there is nothing to override.
    pub fn is_empty(&self) -> bool {
        self.style.is_empty() && self.templates.is_empty()
    }
}

/// One notification type's mail, overridden: templates kept as text,
/// rendered with the mail's own context (plus `style`). An override that
/// fails to render is skipped with a warning, and the type's own template
/// is used.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailOverride {
    /// The subject (`We won {{ title }}!`).
    pub subject: Option<String>,
    /// The HTML body; it may extend the app's layouts
    /// (`{% extends "layouts/mail.html" %}`).
    pub html: Option<String>,
    /// The plain-text body.
    pub text: Option<String>,
}

/// A notification's [`TemplateMail`] with a workspace's style and
/// override applied when it renders.
pub(crate) struct StyledMail<'a> {
    pub(crate) mail: &'a TemplateMail,
    pub(crate) notification_type: &'a str,
    pub(crate) overrides: Option<&'a NotificationOverrides>,
}

impl StyledMail<'_> {
    /// The mail's context with the workspace's `style`.
    fn context(&self) -> Value {
        with_style(&self.mail.context, self.overrides)
    }
}

impl Mailable for StyledMail<'_> {
    fn build(&self, views: &Views) -> Result<Message, MailError> {
        let context = self.context();
        let styled = TemplateMail {
            context: context.clone(),
            ..self.mail.clone()
        };
        let template = self
            .overrides
            .and_then(|overrides| overrides.templates.get(self.notification_type));
        let Some(template) = template else {
            return styled.build(views);
        };
        let render = |kind: &str, source: &Option<String>| -> Option<String> {
            let source = source.as_deref()?;
            let name = format!("{}.{kind}", self.notification_type);
            match views.render_source(&name, source, &context) {
                Ok(rendered) => Some(rendered),
                Err(error) => {
                    tracing::warn!(
                        target: "navree::notify",
                        notification_type = self.notification_type,
                        %error,
                        "a workspace's mail override failed to render; using the default"
                    );
                    None
                }
            }
        };
        let subject = render("txt", &template.subject);
        let html = render("html", &template.html);
        let text = render("txt", &template.text);
        // The parts the override leaves out keep the type's own template.
        let mut message = match styled.build(views) {
            Ok(message) => message,
            Err(error) if html.is_none() && text.is_none() => return Err(error),
            Err(_) => Message {
                to: styled.to.clone(),
                subject: styled.subject.clone(),
                locale: styled
                    .context
                    .get("locale")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                ..Message::default()
            },
        };
        if html.is_some() {
            message.html = html;
        }
        if text.is_some() {
            message.text = text;
        }
        if let Some(subject) = subject {
            message.subject = subject.trim().to_owned();
        }
        Ok(message)
    }

    fn template(&self) -> Option<&str> {
        Some(&self.mail.template)
    }
}

/// `context` with `style` set from `overrides` (when it has none of its
/// own).
pub(crate) fn with_style(context: &Value, overrides: Option<&NotificationOverrides>) -> Value {
    let mut context = match context {
        Value::Object(fields) => fields.clone(),
        Value::Null => Map::new(),
        other => {
            let mut fields = Map::new();
            fields.insert("data".to_owned(), other.clone());
            fields
        }
    };
    if !context.contains_key("style") {
        let style = overrides
            .map(|overrides| Value::Object(overrides.style.clone()))
            .unwrap_or_else(|| json!({}));
        context.insert("style".to_owned(), style);
    }
    Value::Object(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views() -> Views {
        Views::from_templates([
            (
                "mail/deal-won.html",
                "<p{% if style.brand_color %} style=\"color: {{ style.brand_color }}\"{% endif %}>{{ title }} was won.</p>",
            ),
            ("mail/deal-won.txt", "{{ title }} was won."),
            ("layouts/mail.html", "<main>{% block body %}{% endblock %}</main>"),
        ])
        .unwrap()
    }

    fn mail() -> TemplateMail {
        TemplateMail {
            to: vec!["ada@example.com".to_owned()],
            subject: "Deal won: Acme".to_owned(),
            template: "mail/deal-won".to_owned(),
            context: json!({"title": "Acme"}),
        }
    }

    fn overrides(template: Option<MailOverride>) -> NotificationOverrides {
        let mut overrides = NotificationOverrides::default();
        overrides
            .style
            .insert("brand_color".to_owned(), json!("#0F766E"));
        if let Some(template) = template {
            overrides.templates.insert("deal_won".to_owned(), template);
        }
        overrides
    }

    #[test]
    fn the_style_reaches_the_default_template() {
        let mail = mail();
        let overrides = overrides(None);
        let message = StyledMail {
            mail: &mail,
            notification_type: "deal_won",
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.subject, "Deal won: Acme");
        assert_eq!(
            message.html.as_deref(),
            Some("<p style=\"color: #0F766E\">Acme was won.</p>")
        );
        assert_eq!(message.text.as_deref(), Some("Acme was won."));
        // Without overrides, `style` is an empty object.
        let message = StyledMail {
            mail: &mail,
            notification_type: "deal_won",
            overrides: None,
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.html.as_deref(), Some("<p>Acme was won.</p>"));
    }

    #[test]
    fn overrides_replace_the_subject_and_bodies() {
        let mail = mail();
        let overrides = overrides(Some(MailOverride {
            subject: Some("We won {{ title }}!".to_owned()),
            html: Some(
                "{% extends \"layouts/mail.html\" %}{% block body %}<b>{{ title }}</b>{% endblock %}"
                    .to_owned(),
            ),
            text: None,
        }));
        let message = StyledMail {
            mail: &mail,
            notification_type: "deal_won",
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.subject, "We won Acme!");
        assert_eq!(message.html.as_deref(), Some("<main><b>Acme</b></main>"));
        // The text part keeps the type's own template.
        assert_eq!(message.text.as_deref(), Some("Acme was won."));
        assert_eq!(message.to, ["ada@example.com"]);

        // A type whose own template is missing still sends the override.
        let bare = TemplateMail {
            template: "mail/missing".to_owned(),
            ..mail.clone()
        };
        let message = StyledMail {
            mail: &bare,
            notification_type: "deal_won",
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.html.as_deref(), Some("<main><b>Acme</b></main>"));
        assert_eq!(message.to, ["ada@example.com"]);
    }

    #[test]
    fn a_broken_override_falls_back_to_the_default() {
        let mail = mail();
        let overrides = overrides(Some(MailOverride {
            subject: Some("{{ missing }}".to_owned()),
            html: Some("{% if %}".to_owned()),
            text: None,
        }));
        let message = StyledMail {
            mail: &mail,
            notification_type: "deal_won",
            overrides: Some(&overrides),
        }
        .build(&views())
        .unwrap();
        assert_eq!(message.subject, "Deal won: Acme");
        assert_eq!(message.text.as_deref(), Some("Acme was won."));
    }

    #[test]
    fn a_context_with_its_own_style_keeps_it() {
        let overrides = overrides(None);
        let context = with_style(&json!({"style": {"x": 1}}), Some(&overrides));
        assert_eq!(context["style"], json!({"x": 1}));
        assert_eq!(with_style(&Value::Null, None), json!({"style": {}}));
        assert_eq!(
            with_style(&json!([1]), None),
            json!({"data": [1], "style": {}})
        );
        assert!(NotificationOverrides::default().is_empty());
        assert!(!overrides.is_empty());
    }
}
