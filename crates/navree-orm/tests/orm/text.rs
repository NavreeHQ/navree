//! Text matching that ignores letter case on SQLite, whose own `LOWER`,
//! `LIKE` and `NOCASE` fold ASCII letters only: `contains_ignore_case` and
//! `eq_ignore_case` fold every letter with case forms, keep accents, and
//! take the text literally.

use navree_orm::{Filter, Model};

use crate::models::Company;
use crate::support::{TestDb, sqlite};

/// Companies named `names`, in that order, without a country.
async fn companies(test: &TestDb, names: &[&str]) {
    for name in names {
        Company::create(
            &test.db,
            Company {
                name: (*name).to_owned(),
                ..Company::default()
            },
        )
        .await
        .expect("the company is created");
    }
}

/// The names of the companies matching `filter`, in creation order.
async fn names(test: &TestDb, filter: Filter) -> Vec<String> {
    Company::query()
        .where_(filter)
        .order_by(Company::ID)
        .pluck(&test.db, Company::NAME)
        .await
        .expect("the query runs")
}

#[tokio::test]
async fn contains_ignores_case_beyond_ascii_and_keeps_accents() {
    let test = sqlite().await;
    companies(
        &test,
        &["Équipe Lyon", "EQUIPE PARIS", "Ölwechsel GmbH", "Agra"],
    )
    .await;

    for term in ["équipe", "ÉQUIPE", "Équipe", "pe ly"] {
        assert_eq!(
            names(&test, Company::NAME.contains_ignore_case(term)).await,
            ["Équipe Lyon"],
            "{term}"
        );
    }
    // Accents are kept: `equipe` is another word.
    assert_eq!(
        names(&test, Company::NAME.contains_ignore_case("equipe")).await,
        ["EQUIPE PARIS"]
    );
    assert_eq!(
        names(&test, Company::NAME.contains_ignore_case("ölwechsel")).await,
        ["Ölwechsel GmbH"]
    );
    assert!(
        names(&test, Company::NAME.contains_ignore_case("gulati"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn the_text_is_taken_literally() {
    let test = sqlite().await;
    companies(
        &test,
        &["50% Off", "500 Offices", "A_B Ltd", "AXB Ltd", "[x]*?"],
    )
    .await;

    assert_eq!(
        names(&test, Company::NAME.contains_ignore_case("50%")).await,
        ["50% Off"]
    );
    assert_eq!(
        names(&test, Company::NAME.contains_ignore_case("a_b")).await,
        ["A_B Ltd"]
    );
    // GLOB's own wildcards match themselves too.
    assert_eq!(
        names(&test, Company::NAME.contains_ignore_case("[X]*?")).await,
        ["[x]*?"]
    );
}

#[tokio::test]
async fn equality_ignores_case_and_nulls_never_match() {
    let test = sqlite().await;
    companies(&test, &["Agra", "Agra Holdings"]).await;

    assert_eq!(
        names(&test, Company::NAME.eq_ignore_case("AGRA")).await,
        ["Agra"]
    );
    assert!(
        names(&test, Company::NAME.eq_ignore_case("acm"))
            .await
            .is_empty()
    );
    // Every country is NULL: neither condition matches it, nor does its
    // negation.
    assert!(
        names(&test, Company::COUNTRY.contains_ignore_case(""))
            .await
            .is_empty()
    );
    assert!(
        names(&test, Company::COUNTRY.eq_ignore_case("de").not())
            .await
            .is_empty()
    );
}
