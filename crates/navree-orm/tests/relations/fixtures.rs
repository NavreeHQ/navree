//! A small CRM to query: a team of two reps, three companies (one a
//! subsidiary), four deals with activities, contacts, and three tags.

#![allow(dead_code)]

use navree_db::Db;
use navree_orm::prelude::*;

use crate::models::{Activity, Company, Contact, Deal, Tag, Team, User};

/// What [`seed`] created.
pub struct Crm {
    pub team: Record<Team>,
    /// Owns Acme, Renewal and Platform.
    pub ada: Record<User>,
    /// Owns Globex and Upgrade.
    pub bob: Record<User>,
    /// Contacts Alice and Carol; deals Renewal and Upgrade.
    pub acme: Record<Company>,
    /// A subsidiary of Acme, without deals or contacts.
    pub acme_labs: Record<Company>,
    /// Contact Dave; deal Platform.
    pub globex: Record<Company>,
    /// Acme, Ada, 5 000: calls of 30 and 45 minutes and an email of 5.
    pub renewal: Record<Deal>,
    /// Acme, Bob, 25 000: a meeting of 60 minutes.
    pub upgrade: Record<Deal>,
    /// Globex, Ada, 90 000: no activities.
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
    let ada = User::create(db, rep("Ada")).await.unwrap();
    let bob = User::create(db, rep("Bob")).await.unwrap();

    let acme = Company::create(
        db,
        Company {
            owner_id: Some(ada.id),
            ..Company::named("Acme")
        },
    )
    .await
    .unwrap();
    let acme_labs = Company::create(
        db,
        Company {
            parent_id: Some(acme.id),
            ..Company::named("Acme Labs")
        },
    )
    .await
    .unwrap();
    let globex = Company::create(
        db,
        Company {
            owner_id: Some(bob.id),
            ..Company::named("Globex")
        },
    )
    .await
    .unwrap();

    let deal = |title: &str, cents: i64, company: Option<i64>, owner: Option<i64>| Deal {
        company_id: company,
        owner_id: owner,
        ..Deal::new(title, cents)
    };
    let renewal = Deal::create(db, deal("Renewal", 5_000, Some(acme.id), Some(ada.id)))
        .await
        .unwrap();
    let upgrade = Deal::create(db, deal("Upgrade", 25_000, Some(acme.id), Some(bob.id)))
        .await
        .unwrap();
    let platform = Deal::create(db, deal("Platform", 90_000, Some(globex.id), Some(ada.id)))
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
        (acme.id, "Alice", "alice@acme.test"),
        (acme.id, "Carol", "carol@acme.test"),
        (globex.id, "Dave", "dave@globex.test"),
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
        acme,
        acme_labs,
        globex,
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
