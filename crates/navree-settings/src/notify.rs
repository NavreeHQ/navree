//! Feature `notify`: how notification mail looks, as settings.
//! [`NotificationTemplates`] (key `notification-templates`) holds a
//! workspace's logo, brand color, footer and its own subject and body per
//! notification type; [`SettingsTemplateOverrides`] gives them to
//! navree-notify's `Notifier` as its `dyn TemplateOverrides`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use garde::Validate;
use navree_contracts::BoxFuture;
use navree_kernel::BoxError;
use navree_notify::{MailOverride, NotificationOverrides, TemplateOverrides};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{Scope, ScopeKind, Settings, SettingsGroup};

/// The longest subject override.
pub const MAX_SUBJECT_LEN: usize = 500;

/// The longest body override (HTML or text).
pub const MAX_BODY_LEN: usize = 100_000;

/// How notification mail looks: the style every mail gets as `style`
/// (`{{ style.logo_url }}`, `{{ style.brand_color }}`, `{{ style.footer }}`
/// and the extra `style` keys) and the subject and body overrides by
/// notification type (`deal_won`; `notify.digest` for the digest mail).
///
/// Stored at the global, organization and workspace levels: a workspace
/// inherits each field from its organization, which inherits from the
/// global values. Change it with `PATCH
/// /api/settings/notification-templates?scope=workspace` (permission
/// `workspace.update`) or [`Settings::update`].
///
/// ```
/// use navree_settings::notify::{NotificationTemplates, TemplateOverride};
///
/// let templates = NotificationTemplates {
///     brand_color: Some("#0F766E".into()),
///     footer: Some("Acme Corp, 12 Harbour Road, Pune".into()),
///     templates: [(
///         "deal_won".to_owned(),
///         TemplateOverride { subject: Some("We won {{ title }}!".into()), ..TemplateOverride::default() },
///     )]
///     .into(),
///     ..NotificationTemplates::default()
/// };
/// let overrides = templates.overrides();
/// assert_eq!(overrides.style["brand_color"], "#0F766E");
/// assert_eq!(overrides.templates["deal_won"].subject.as_deref(), Some("We won {{ title }}!"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct NotificationTemplates {
    /// The logo at the top of notification mail: an `https` URL.
    #[garde(inner(custom(https_url)))]
    pub logo_url: Option<String>,
    /// The brand color, `#RRGGBB`.
    #[garde(inner(custom(hex_color)))]
    pub brand_color: Option<String>,
    /// The footer line (an address, a note on why the mail came).
    #[garde(inner(length(max = 1000)))]
    pub footer: Option<String>,
    /// More style variables for the templates (`{{ style.accent }}`).
    #[garde(custom(style_keys))]
    pub style: BTreeMap<String, String>,
    /// Subject and body overrides by notification type.
    #[garde(custom(overrides_fit))]
    pub templates: BTreeMap<String, TemplateOverride>,
}

/// One notification type's mail, overridden: templates kept as text,
/// rendered with the mail's own variables plus `style`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateOverride {
    /// The subject (`We won {{ title }}!`).
    pub subject: Option<String>,
    /// The HTML body; it may extend the app's layouts.
    pub html: Option<String>,
    /// The plain-text body.
    pub text: Option<String>,
}

impl SettingsGroup for NotificationTemplates {
    const KEY: &'static str = "notification-templates";
    const SCOPES: &'static [ScopeKind] = &[
        ScopeKind::Global,
        ScopeKind::Organization,
        ScopeKind::Workspace,
    ];
    const DESCRIPTION: &'static str = "How notification mail looks: logo, brand color, footer, \
                                       and subject and body overrides per notification type.";
}

impl NotificationTemplates {
    /// These settings as navree-notify's overrides: the style variables
    /// (the named fields over the extra ones) and the templates.
    pub fn overrides(&self) -> NotificationOverrides {
        let mut style: serde_json::Map<String, Value> = self
            .style
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();
        for (key, value) in [
            ("logo_url", &self.logo_url),
            ("brand_color", &self.brand_color),
            ("footer", &self.footer),
        ] {
            if let Some(value) = value {
                style.insert(key.to_owned(), Value::String(value.clone()));
            }
        }
        let templates = self
            .templates
            .iter()
            .map(|(notification_type, template)| {
                (
                    notification_type.clone(),
                    MailOverride {
                        subject: template.subject.clone(),
                        html: template.html.clone(),
                        text: template.text.clone(),
                    },
                )
            })
            .collect();
        NotificationOverrides { style, templates }
    }
}

/// navree-notify's `dyn TemplateOverrides` read from the
/// [`NotificationTemplates`] settings: a workspace's resolved values (its
/// own over its organization's, over the global ones), or the global
/// values for mail sent outside any workspace. Registered by
/// `SettingsModule` with the `notify` feature.
pub struct SettingsTemplateOverrides {
    settings: Arc<Settings>,
}

impl SettingsTemplateOverrides {
    /// Overrides read through `settings`.
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }
}

impl fmt::Debug for SettingsTemplateOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsTemplateOverrides")
            .finish_non_exhaustive()
    }
}

impl TemplateOverrides for SettingsTemplateOverrides {
    fn overrides(
        &self,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'_, Result<NotificationOverrides, BoxError>> {
        Box::pin(async move {
            let scope = workspace.map_or_else(Scope::global, Scope::workspace);
            let templates = self.settings.get::<NotificationTemplates>(scope).await?;
            Ok(templates.overrides())
        })
    }
}

/// An `https` URL (or `http` on localhost).
fn https_url<T: AsRef<str> + ?Sized>(value: &T, _ctx: &()) -> garde::Result {
    let value = value.as_ref();
    let parsed =
        url::Url::parse(value.trim()).map_err(|_| garde::Error::new("must be an https URL"))?;
    let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"));
    if parsed.scheme() == "https" || (parsed.scheme() == "http" && local) {
        Ok(())
    } else {
        Err(garde::Error::new("must be an https URL"))
    }
}

/// `#RRGGBB`.
fn hex_color<T: AsRef<str> + ?Sized>(value: &T, _ctx: &()) -> garde::Result {
    let value = value.as_ref();
    let hex = value.strip_prefix('#').unwrap_or("");
    if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(garde::Error::new("must be a color written #RRGGBB"))
    }
}

/// Style keys are template names (`accent`), values short.
fn style_keys(value: &BTreeMap<String, String>, _ctx: &()) -> garde::Result {
    for (key, text) in value {
        let valid = !key.is_empty()
            && key.len() <= 64
            && key.starts_with(|c: char| c.is_ascii_lowercase())
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(garde::Error::new(format!(
                "`{key}` is not a style name (lower-case letters, digits and `_`)"
            )));
        }
        if text.len() > 1000 {
            return Err(garde::Error::new(format!(
                "`{key}` is longer than 1000 characters"
            )));
        }
    }
    Ok(())
}

/// Overrides of known sizes, for notification types with plain names.
fn overrides_fit(value: &BTreeMap<String, TemplateOverride>, _ctx: &()) -> garde::Result {
    for (notification_type, template) in value {
        if navree_notify::check_category(notification_type).is_err() {
            return Err(garde::Error::new(format!(
                "`{notification_type}` is not a notification type"
            )));
        }
        if template
            .subject
            .as_ref()
            .is_some_and(|subject| subject.chars().count() > MAX_SUBJECT_LEN)
        {
            return Err(garde::Error::new(format!(
                "`{notification_type}`: the subject is longer than {MAX_SUBJECT_LEN} characters"
            )));
        }
        for body in [&template.html, &template.text].into_iter().flatten() {
            if body.len() > MAX_BODY_LEN {
                return Err(garde::Error::new(format!(
                    "`{notification_type}`: a body is longer than {MAX_BODY_LEN} bytes"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(templates: &NotificationTemplates) -> Result<(), String> {
        templates.validate().map_err(|errors| errors.to_string())
    }

    #[test]
    fn the_named_fields_win_over_the_extra_style() {
        let templates = NotificationTemplates {
            brand_color: Some("#0F766E".into()),
            style: [
                ("brand_color".to_owned(), "#000000".to_owned()),
                ("accent".to_owned(), "#F59E0B".to_owned()),
            ]
            .into(),
            ..NotificationTemplates::default()
        };
        let overrides = templates.overrides();
        assert_eq!(overrides.style["brand_color"], "#0F766E");
        assert_eq!(overrides.style["accent"], "#F59E0B");
        assert!(overrides.templates.is_empty());
        assert!(NotificationTemplates::default().overrides().is_empty());
    }

    #[test]
    fn values_are_checked() {
        assert!(valid(&NotificationTemplates::default()).is_ok());
        let bad = [
            NotificationTemplates {
                brand_color: Some("teal".into()),
                ..NotificationTemplates::default()
            },
            NotificationTemplates {
                logo_url: Some("http://acme.test/logo.png".into()),
                ..NotificationTemplates::default()
            },
            NotificationTemplates {
                style: [("Bad Key".to_owned(), "x".to_owned())].into(),
                ..NotificationTemplates::default()
            },
            NotificationTemplates {
                templates: [(
                    "deal_won".to_owned(),
                    TemplateOverride {
                        subject: Some("x".repeat(MAX_SUBJECT_LEN + 1)),
                        ..TemplateOverride::default()
                    },
                )]
                .into(),
                ..NotificationTemplates::default()
            },
            NotificationTemplates {
                templates: [("deal won".to_owned(), TemplateOverride::default())].into(),
                ..NotificationTemplates::default()
            },
        ];
        for templates in bad {
            assert!(valid(&templates).is_err(), "{templates:?}");
        }
        let good = NotificationTemplates {
            logo_url: Some("https://cdn.acme.test/logo.png".into()),
            brand_color: Some("#0f766e".into()),
            footer: Some("Acme".into()),
            ..NotificationTemplates::default()
        };
        assert!(valid(&good).is_ok());
    }
}
