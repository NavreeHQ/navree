//! Global scopes of the related model apply to relation queries, eager
//! loads, existence subqueries, relation aggregates and pagination counts.

use navree_orm::prelude::*;

use crate::fixtures::seed;
use crate::models::{Company, Contact, Deal, Visible};
use crate::support::sqlite;

fn hidden(name: &str) -> Contact {
    Contact {
        name: name.to_owned(),
        email: format!("{}@hidden.test", name.to_lowercase()),
        ..Contact::default()
    }
}

#[tokio::test]
async fn global_scopes_apply_wherever_relations_query() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.agra
        .contacts()
        .create(&t.db, hidden("Ghost"))
        .await
        .unwrap();
    let ghost_town = Company::create(&t.db, Company::named("Ghost Town"))
        .await
        .unwrap();
    ghost_town
        .contacts()
        .create(&t.db, hidden("Phantom"))
        .await
        .unwrap();

    // Relation queries.
    assert_eq!(crm.agra.contacts().count(&t.db).await.unwrap(), 2);
    assert_eq!(
        crm.agra
            .contacts()
            .without_global_scope::<Visible>()
            .count(&t.db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        crm.agra
            .contacts()
            .without_global_scopes()
            .get(&t.db)
            .await
            .unwrap()
            .len(),
        3
    );
    // Pagination counts.
    let page = crm.agra.contacts().paginate(&t.db, (1, 10)).await.unwrap();
    assert_eq!((page.meta.total, page.data.len()), (2, 2));

    // Eager loads and aggregates.
    let companies = Company::query()
        .with("contacts")
        .with_count("contacts")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(companies[0].loaded(Company::contacts).unwrap().len(), 2);
    assert_eq!(
        companies[0].extra::<i64>("contacts_count").unwrap(),
        Some(2)
    );
    assert_eq!(
        companies[3].extra::<i64>("contacts_count").unwrap(),
        Some(0)
    );
    let unscoped = Company::query()
        .with_count_constrained(Company::contacts, |q| q.without_global_scopes())
        .where_key(ghost_town.id)
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unscoped.extra::<i64>("contacts_count").unwrap(), Some(1));

    // Existence subqueries.
    let with_contacts: Vec<String> = Company::query()
        .has("contacts")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap()
        .iter()
        .map(|company| company.name.clone())
        .collect();
    assert_eq!(with_contacts, ["Agra", "Gulati"]);
    let any = Company::query()
        .where_has(Company::contacts, |q| q.without_global_scope::<Visible>())
        .count(&t.db)
        .await
        .unwrap();
    assert_eq!(any, 3);

    // Through relations, and nested paths.
    assert_eq!(crm.renewal.contacts().count(&t.db).await.unwrap(), 2);
    let reachable = Deal::query()
        .where_relation("company.contacts", Contact::NAME.eq("Ghost"))
        .count(&t.db)
        .await
        .unwrap();
    assert_eq!(reachable, 0);
}
