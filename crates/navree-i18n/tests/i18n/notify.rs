//! Notifications in the recipient's saved locale.

use navree_db::Db;
use navree_i18n::{I18n, t};
use navree_kernel::{Module, ModuleBuilder};
use navree_mail::{MailModule, MailOptions, Mailer, TemplateMail, TransportConfig};
use navree_notify::{Channel, Notifiable, Notification, Notifier, NotifyModule, NotifyOptions, notifications};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions};
use sea_orm::{EntityTrait, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, World};

/// A user that receives notifications.
struct Recipient {
    id: Uuid,
}

impl Notifiable for Recipient {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        Some(format!("{}@navree.test", self.id.simple()))
    }
}

/// The docs' deal-won notification, translated where it is built.
#[derive(Debug, Serialize, Deserialize)]
struct DealWon {
    title: String,
}

impl Notification for DealWon {
    const TYPE: &'static str = "deal_won";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![Channel::Database, Channel::Mail]
    }

    fn to_mail(&self, _notifiable: &dyn Notifiable) -> Option<TemplateMail> {
        Some(TemplateMail {
            subject: t!("deal-won-subject", title = self.title.as_str()),
            template: "mail/won".to_owned(),
            context: json!({ "title": self.title }),
            ..TemplateMail::default()
        })
    }

    fn to_database(&self, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "message": t!("deal-won", owner = "Ada", title = self.title.as_str()) }))
    }
}

struct Templates;

impl Module for Templates {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/won.txt", "{{ t(\"mail-footer\") }}")]);
    }
}

#[tokio::test]
async fn notifications_render_in_the_recipients_saved_locale() {
    let world = World::with(
        support::with_database()
            .module(ViewModule::for_root(ViewOptions {
                dirs: vec![],
                reload: false,
            }))
            .module(Templates)
            .module(MailModule::for_root(MailOptions {
                from: "Navree <noreply@navree.test>".to_owned(),
                transport: TransportConfig::Array,
                queue: None,
            }))
            .module(NotifyModule::for_root(NotifyOptions {
                routes: false,
                ..NotifyOptions::default()
            })),
    )
    .await;
    let french = world.user("Camille").await;
    let unsaved = world.user("Dana").await;
    let i18n = world.app.get::<I18n>().await;
    world
        .work(|uow| {
            let i18n = i18n.clone();
            async move { i18n.set_user_locale(&uow, french, "fr").await }
        })
        .await
        .unwrap();

    // A German manager notifies both: each reads their own locale.
    let notifier = world.app.get::<Notifier>().await;
    i18n.scope("de", async {
        for id in [french, unsaved] {
            notifier
                .send(&Recipient { id }, DealWon { title: "Lovelace renewal".into() })
                .await
                .unwrap();
        }
    })
    .await;

    let mail = world.app.get::<Mailer>().await.array_transport().unwrap().sent();
    assert_eq!(mail[0].subject, "Affaire gagnée : Lovelace renewal");
    assert_eq!(mail[0].text.as_deref(), Some("Envoyé par Acme CRM."));
    assert_eq!(mail[1].subject, "You won Lovelace renewal");
    assert_eq!(mail[1].text.as_deref(), Some("Sent by Acme CRM."));

    let db = world.app.get::<Db>().await;
    let rows = notifications::Entity::find()
        .order_by_asc(notifications::Column::CreatedAt)
        .order_by_asc(notifications::Column::Id)
        .all(db.conn())
        .await
        .unwrap();
    assert_eq!(rows[0].data, json!({ "message": "Ada a gagné Lovelace renewal." }));
    assert_eq!(rows[1].data, json!({ "message": "Ada won Lovelace renewal." }));
    world.app.shutdown().await.unwrap();
}
