//! Feature `notify`: the `notification-templates` settings restyle the
//! notification mail of a workspace, inherited from its organization and
//! the global values.

use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, TemplateMail, TransportConfig};
use navree_notify::{
    Channel, Notifiable, Notification, Notifier, NotifyModule, NotifyOptions, TemplateOverrides,
};
use navree_settings::notify::NotificationTemplates;
use navree_settings::{Scope, Settings, SettingsRegistry};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::support::{World, builder};

/// The mail templates of `DealWon`.
struct Templates;

impl Module for Templates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[
            (
                "mail/deal-won.html",
                "<p{% if style.brand_color %} style=\"color: {{ style.brand_color }}\"{% endif %}>{{ title }} was won.</p>",
            ),
            (
                "mail/deal-won.txt",
                "{{ title }} was won.{% if style.footer %}\n{{ style.footer }}{% endif %}",
            ),
        ]);
    }
}

#[derive(Serialize, Deserialize)]
struct DealWon {
    title: String,
}

impl Notification for DealWon {
    const TYPE: &'static str = "deal_won";

    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Mail]
    }

    fn to_mail(&self, _n: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: format!("Deal won: {}", self.title),
            template: "mail/deal-won".to_owned(),
            context: json!({ "title": self.title }),
            ..TemplateMail::default()
        })
    }
}

/// A recipient in a workspace (or none).
struct Rep {
    id: Uuid,
    workspace: Option<Uuid>,
}

impl Notifiable for Rep {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(format!("rep-{}@navree.test", self.id.simple()))
    }

    fn workspace_id(&self) -> Option<Uuid> {
        self.workspace
    }
}

async fn world() -> World {
    World::with(
        builder()
            .module(ViewModule::for_root(ViewOptions {
                dirs: vec![],
                reload: false,
            }))
            .module(Templates)
            .module(MailModule::for_root(MailOptions {
                from: "Navree <noreply@navree.test>".to_owned(),
                transport: TransportConfig::Array,
                ..MailOptions::default()
            }))
            .module(NotifyModule::for_root(NotifyOptions {
                routes: false,
                ..NotifyOptions::default()
            })),
    )
    .await
}

#[tokio::test]
async fn the_module_registers_the_group_and_the_overrides() {
    let world = world().await;
    let registry = world.app.get::<SettingsRegistry>().await;
    assert!(registry.get("notification-templates").is_some());
    // navree-notify finds them.
    assert!(
        world
            .app
            .ctx()
            .try_get::<dyn TemplateOverrides>()
            .await
            .is_some()
    );
}

#[tokio::test]
async fn a_workspace_inherits_its_organizations_style_and_overrides_its_own_mail() {
    let world = world().await;
    let owner = world.user("Owner").await;
    let holding = world.organization(owner, "Holding").await;
    let agra = world.workspace(owner, "Agra").await;
    world.attach(holding.id, agra.id).await;
    let other = world.workspace(owner, "Other").await;
    let settings: std::sync::Arc<Settings> = world.settings().await;

    let (org_scope, agra_scope) = (Scope::organization(holding.id), Scope::workspace(agra.id));
    let global = settings.clone();
    world
        .work(|uow| async move {
            global
                .update::<NotificationTemplates>(
                    &uow,
                    Scope::global(),
                    json!({"footer": "Sent by Navree"}),
                )
                .await?;
            global
                .update::<NotificationTemplates>(
                    &uow,
                    org_scope,
                    json!({"brand_color": "#0F766E", "footer": "Holding Group, Pune"}),
                )
                .await?;
            global
                .update::<NotificationTemplates>(
                    &uow,
                    agra_scope,
                    json!({"templates": {"deal_won": {"subject": "Agra won {{ title }}!"}}}),
                )
                .await
        })
        .await
        .unwrap();

    let notifier = world.app.get::<Notifier>().await;
    for workspace in [Some(agra.id), Some(other.id), None] {
        notifier
            .send(
                &Rep {
                    id: Uuid::now_v7(),
                    workspace,
                },
                DealWon {
                    title: "Indrani".into(),
                },
            )
            .await
            .unwrap();
    }

    let sent = world
        .app
        .get::<Mailer>()
        .await
        .array_transport()
        .unwrap()
        .sent();
    assert_eq!(sent.len(), 3);
    // Agra: its own subject, its organization's color and footer.
    assert_eq!(sent[0].subject, "Agra won Indrani!");
    assert_eq!(
        sent[0].html.as_deref(),
        Some("<p style=\"color: #0F766E\">Indrani was won.</p>")
    );
    assert_eq!(
        sent[0].text.as_deref(),
        Some("Indrani was won.\nHolding Group, Pune")
    );
    // Another workspace and mail outside any: the global values.
    for mail in &sent[1..] {
        assert_eq!(mail.subject, "Deal won: Indrani");
        assert_eq!(mail.html.as_deref(), Some("<p>Indrani was won.</p>"));
        assert_eq!(
            mail.text.as_deref(),
            Some("Indrani was won.\nSent by Navree")
        );
    }
}

#[tokio::test]
async fn invalid_templates_are_refused() {
    let world = world().await;
    let settings = world.settings().await;
    let error = world
        .work(|uow| async move {
            settings
                .update::<NotificationTemplates>(
                    &uow,
                    Scope::global(),
                    json!({"brand_color": "teal"}),
                )
                .await
        })
        .await
        .unwrap_err();
    match error {
        navree_settings::SettingsError::Invalid(errors) => {
            assert_eq!(
                errors.get("brand_color").unwrap(),
                ["must be a color written #RRGGBB"]
            );
        }
        other => panic!("not refused as invalid: {other}"),
    }
}
