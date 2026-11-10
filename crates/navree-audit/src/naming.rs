//! Subject types and actions derived from table names.

/// The default subject type of a table: its name singularised, in
/// PascalCase (`deals` → `Deal`, `companies` → `Company`, `deal_notes` →
/// `DealNote`). Only the last word is singularised, with simple English
/// rules; map irregular names with `AuditOptions::subject_types`.
pub fn subject_type_of(table: &str) -> String {
    let words: Vec<&str> = table.split('_').filter(|w| !w.is_empty()).collect();
    let last = words.len().saturating_sub(1);
    words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            if i == last {
                capitalize(&singular(word))
            } else {
                capitalize(word)
            }
        })
        .collect()
}

/// `PascalCase` → `snake_case` (`DealNote` → `deal_note`).
pub(crate) fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let previous = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if previous != '_'
                && (previous.is_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_uppercase() && next_is_lower))
            {
                out.push('_');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// One English word in the singular (simple rules only).
fn singular(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let strip = |n: usize| word[..word.len() - n].to_owned();
    if lower.len() > 3 && lower.ends_with("ies") {
        format!("{}y", strip(3))
    } else if ["sses", "shes", "ches", "xes", "zzes"]
        .iter()
        .any(|end| lower.ends_with(end))
    {
        strip(2)
    } else if ["ss", "us", "is"].iter().any(|end| lower.ends_with(end)) {
        word.to_owned()
    } else if lower.len() > 1 && lower.ends_with('s') {
        strip(1)
    } else {
        word.to_owned()
    }
}

/// The word with its first letter in upper case.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_types_are_singular_pascal_case() {
        for (table, subject) in [
            ("deals", "Deal"),
            ("companies", "Company"),
            ("activities", "Activity"),
            ("users", "User"),
            ("contacts", "Contact"),
            ("deal_notes", "DealNote"),
            ("addresses", "Address"),
            ("boxes", "Box"),
            ("matches", "Match"),
            ("wishes", "Wish"),
            ("access", "Access"),
            ("data", "Data"),
            ("Deals", "Deal"),
            ("days", "Day"),
        ] {
            assert_eq!(subject_type_of(table), subject, "table `{table}`");
        }
    }

    #[test]
    fn snake_case_splits_words() {
        assert_eq!(snake_case("Deal"), "deal");
        assert_eq!(snake_case("DealNote"), "deal_note");
        assert_eq!(snake_case("deal_note"), "deal_note");
        assert_eq!(snake_case("APIKey"), "api_key");
        assert_eq!(snake_case("User2Fa"), "user2_fa");
    }
}
