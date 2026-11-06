//! Text matching that ignores letter case on the real servers: the same
//! results as on SQLite, whatever the column's collation (MySQL's default
//! `utf8mb4_0900_ai_ci` would also ignore accents).

use navree_db::Db;
use navree_orm::{Filter, Model};

use crate::models::Company;
use crate::support::Server;

/// The names of the companies matching `filter`, in creation order.
async fn names(db: &Db, filter: Filter) -> Vec<String> {
    Company::query()
        .where_(filter)
        .order_by(Company::ID)
        .pluck(db, Company::NAME)
        .await
        .expect("the query runs")
}

/// `contains_ignore_case` and `eq_ignore_case`: case is ignored for every
/// letter, accents are kept, wildcards in the text match themselves.
pub async fn ignoring_case(db: Db, server: Server) {
    for name in [
        "Équipe Lyon",
        "EQUIPE PARIS",
        "Ölwechsel GmbH",
        "50% Off",
        "500 Offices",
        "A_B Ltd",
        "AXB Ltd",
        "Agra",
    ] {
        Company::create(
            &db,
            Company {
                name: name.to_owned(),
                ..Company::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{server:?}: {name}: {error}"));
    }

    for term in ["équipe", "ÉQUIPE", "pe ly"] {
        assert_eq!(
            names(&db, Company::NAME.contains_ignore_case(term)).await,
            ["Équipe Lyon"],
            "{server:?}: {term}"
        );
    }
    assert_eq!(
        names(&db, Company::NAME.contains_ignore_case("equipe")).await,
        ["EQUIPE PARIS"],
        "{server:?}: accents are kept"
    );
    assert_eq!(
        names(&db, Company::NAME.contains_ignore_case("ÖLWECHSEL")).await,
        ["Ölwechsel GmbH"],
        "{server:?}"
    );
    assert_eq!(
        names(&db, Company::NAME.contains_ignore_case("50%")).await,
        ["50% Off"],
        "{server:?}: `%` is literal"
    );
    assert_eq!(
        names(&db, Company::NAME.contains_ignore_case("a_b")).await,
        ["A_B Ltd"],
        "{server:?}: `_` is literal"
    );
    assert_eq!(
        names(&db, Company::NAME.eq_ignore_case("AGRA")).await,
        ["Agra"],
        "{server:?}"
    );
    assert!(
        names(&db, Company::COUNTRY.contains_ignore_case(""))
            .await
            .is_empty(),
        "{server:?}: NULLs never match"
    );
}
