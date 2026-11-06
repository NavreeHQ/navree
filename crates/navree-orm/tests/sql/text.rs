//! Text matching that ignores letter case on every database, non-ASCII
//! letters included, with the text taken literally: SQLite matches with a
//! `GLOB` listing each letter's case variants, MySQL compares lower-cased
//! bytes, PostgreSQL keeps `LOWER(..) LIKE` with escaped wildcards.

use navree_orm::Model;

use super::{Company, query};

#[test]
fn text_matches_ignoring_case_literally() {
    insta::assert_snapshot!(query(
        Company::query()
            .where_(Company::NAME.contains_ignore_case("Équipe 50%_"))
            .or_where(Company::COUNTRY.eq_ignore_case("Österreich"))
    ));
}

#[test]
fn ignoring_case_combines_with_other_conditions() {
    insta::assert_snapshot!(query(
        Company::query()
            .where_(Company::ID.gt(3_i64))
            .where_(
                Company::NAME
                    .contains_ignore_case("agra")
                    .or(Company::COUNTRY.contains_ignore_case("agra"))
            )
            .where_not(Company::NAME.eq_ignore_case("ACME Holdings"))
    ));
}
