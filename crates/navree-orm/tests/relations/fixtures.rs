//! A small CRM to query: a team of two reps, three companies (one a
//! subsidiary), four deals with activities, contacts, and three tags.

#![allow(dead_code)]

use navree_db::Db;
use navree_orm::prelude::*;

use crate::models::{Activity, Company, Contact, Deal, Tag, Team, User};

/// What [`seed`] created.
pub struct Crm {
    pub team: Record<Team>,
    /// Owns Agra, Renewal and Platform.
    pub ada: Record<User>,
    /// Owns Gulati and Upgrade.
    pub bob: Record<User>,
    /// Contacts Aarti and Charu; deals Renewal and Upgrade.
    pub agra: Record<Company>,
    /// A subsidiary of Agra, without deals or contacts.
    pub agra_labs: Record<Company>,
    /// Contact Deep; deal Platform.
    pub gulati: Record<Company>,
    /// Agra, Anu, 5 000: calls of 30 and 45 minutes and an email of 5.
    pub renewal: Record<Deal>,
    /// Agra, Bal, 25 000: a meeting of 60 minutes.
    pub upgrade: Record<Deal>,
    /// Gulati, Anu, 90 000: no activities.
    pub platform: Record<Deal>,
    /// No company, no owner, 50.
    pub orphan: Record<Deal>,
    pub vip: Record<Tag>,
    pub q1: Record<Tag>,
    pub hot: Record<Tag>,
}

/// Creates the CRM.
pub async fn seed(db: &Db) -> Crm {
    let team = Team::create(
        db,
        Team {
            name: "Enterprise".into(),
            ..Team::default()
        },
    )
    .await
    .unwrap();
    let rep = |name: &str| User {
        team_id: Some(team.id),
        name: name.to_owned(),
        ..User::default()
    };
    let ada = User::create(db, rep("Anu")).await.unwrap();
    let bob = User::create(db, rep("Bal")).await.unwrap();

    let agra = Company::create(
        db,
        Company {
            owner_id: Some(ada.id),
            ..Company::named("Agra")
        },
    )
    .await
    .unwrap();
    let agra_labs = Company::create(
        db,
        Company {
            parent_id: Some(agra.id),
            ..Company::named("Agra Labs")
        },
    )
    .await
    .unwrap();
    let gulati = Company::create(
        db,
        Company {
            owner_id: Some(bob.id),
            ..Company::named("Gulati")
        },
    )
    .await
    .unwrap();

    let deal = |title: &str, cents: i64, company: Option<i64>, owner: Option<i64>| Deal {
        company_id: company,
        owner_id: owner,
        ..Deal::new(title, cents)
    };
    let renewal = Deal::create(db, deal("Renewal", 5_000, Some(agra.id), Some(ada.id)))
        .await
        .unwrap();
    let upgrade = Deal::create(db, deal("Upgrade", 25_000, Some(agra.id), Some(bob.id)))
        .await
        .unwrap();
    let platform = Deal::create(db, deal("Platform", 90_000, Some(gulati.id), Some(ada.id)))
        .await
        .unwrap();
    let orphan = Deal::create(db, deal("Orphan", 50, None, None))
        .await
        .unwrap();

    for (deal_id, kind, minutes) in [
        (renewal.id, "call", 30),
        (renewal.id, "email", 5),
        (renewal.id, "call", 45),
        (upgrade.id, "meeting", 60),
    ] {
        Activity::create(
            db,
            Activity {
                deal_id: Some(deal_id),
                ..Activity::new(kind, minutes)
            },
        )
        .await
        .unwrap();
    }

    for (company_id, name, email) in [
        (agra.id, "Aarti", "aarti@acme.test"),
        (agra.id, "Charu", "charu@acme.test"),
        (gulati.id, "Deep", "deep@gulati.test"),
    ] {
        Contact::create(
            db,
            Contact {
                company_id: Some(company_id),
                name: name.into(),
                email: email.into(),
                ..Contact::default()
            },
        )
        .await
        .unwrap();
    }

    let vip = Tag::create(db, Tag::named("vip")).await.unwrap();
    let q1 = Tag::create(db, Tag::named("q1")).await.unwrap();
    let hot = Tag::create(db, Tag::named("hot")).await.unwrap();

    // Records as stored now (activities touched their deals).
    let renewal = Deal::find_or_fail(db, renewal.id).await.unwrap();
    let upgrade = Deal::find_or_fail(db, upgrade.id).await.unwrap();

    Crm {
        team,
        ada,
        bob,
        agra,
        agra_labs,
        gulati,
        renewal,
        upgrade,
        platform,
        orphan,
        vip,
        q1,
        hot,
    }
}

/// The titles of `deals`, in order.
pub fn titles(deals: &Collection<Deal>) -> Vec<String> {
    deals.iter().map(|deal| deal.title.clone()).collect()
}

/// The names of `companies`, in order.
pub fn names(companies: &Collection<Company>) -> Vec<String> {
    companies
        .iter()
        .map(|company| company.name.clone())
        .collect()
}
