//! Eager loads limited to the records a viewer may see.

use std::sync::{Arc, Mutex};

use navree_orm::prelude::*;
use navree_orm::relations::AccessFilter;
use navree_orm::{ModelMeta, OrmError, Result};

use crate::fixtures::{seed, titles};
use crate::models::{Activity, Company, Deal, Note};
use crate::support::sqlite;

/// A rep who sees the deals they own, every activity but emails, and the
/// companies other than Globex; it notes what it was asked.
#[derive(Clone)]
struct Rep {
    user: i64,
    asked: Arc<Mutex<Vec<String>>>,
}

impl AccessFilter for Rep {
    fn filter(&self, action: &str, model: &'static ModelMeta) -> Result<Option<Filter>> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("{action} {}", model.name()));
        Ok(match model.name() {
            "Deal" => Some(Deal::OWNER_ID.eq(Some(self.user))),
            "Activity" => Some(Activity::KIND.ne("email")),
            "Company" => Some(Company::NAME.ne("Globex")),
            _ => None,
        })
    }
}

#[tokio::test]
async fn eager_loads_keep_what_the_viewer_may_read() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let ada = Rep {
        user: crm.ada.id,
        asked: Arc::default(),
    };

    // Every step of the path is limited.
    let acme = Company::query()
        .with_accessible("deals.activities", &ada)
        .where_key(crm.acme.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    let deals = acme.loaded(Company::deals).unwrap();
    assert_eq!(titles(deals), ["Renewal"], "Bob's Upgrade is hidden");
    let kinds: Vec<&str> = deals[0]
        .loaded(Deal::activities)
        .unwrap()
        .iter()
        .map(|activity| activity.kind.as_str())
        .collect();
    assert_eq!(kinds, ["call", "call"], "emails are hidden");
    assert_eq!(
        *ada.asked.lock().unwrap(),
        ["read Deal", "read Activity"]
    );

    // Another action, and a belongs-to.
    let deals = Deal::query()
        .with_accessible_for(Deal::company, "update", &ada)
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert!(deals[2].loaded(Deal::company).unwrap().is_none(), "Globex");
    assert!(deals[0].loaded(Deal::company).unwrap().is_some());
    assert_eq!(ada.asked.lock().unwrap().last().unwrap(), "update Company");

    // A closure, and a `morph_to` (asked for each model it loads).
    crm.globex
        .notes()
        .create(&t.db, Note::new("visit"))
        .await
        .unwrap();
    crm.acme
        .notes()
        .create(&t.db, Note::new("lunch"))
        .await
        .unwrap();
    let hide_globex = |_: &str, model: &'static ModelMeta| -> Result<Option<Filter>> {
        Ok((model.name() == "Company").then(|| Company::NAME.ne("Globex")))
    };
    let notes = Note::query()
        .with_accessible("notable", &hide_globex)
        .order_by(Note::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert!(notes[0].loaded(Note::notable).unwrap().is_none());
    assert!(notes[1].loaded(Note::notable).unwrap().is_some());

    // A refusal fails the load.
    let refuse = |_: &str, _: &'static ModelMeta| -> Result<Option<Filter>> {
        Err(OrmError::InvalidQuery("not signed in".into()))
    };
    let error = Company::query()
        .with_accessible("deals", &refuse)
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not signed in"), "{error}");
}
