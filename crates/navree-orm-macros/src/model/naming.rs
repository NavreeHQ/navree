//! A model's default table name: the snake_case plural of its name.

use heck::ToSnakeCase;

/// Words that are the same in the plural.
const UNCOUNTABLE: &[&str] = &[
    "audio",
    "data",
    "deer",
    "equipment",
    "feedback",
    "fish",
    "information",
    "metadata",
    "money",
    "news",
    "rice",
    "series",
    "sheep",
    "species",
    "staff",
    "traffic",
];

/// Plurals that follow no rule.
const IRREGULAR: &[(&str, &str)] = &[
    ("analysis", "analyses"),
    ("child", "children"),
    ("criterion", "criteria"),
    ("foot", "feet"),
    ("goose", "geese"),
    ("half", "halves"),
    ("index", "indices"),
    ("knife", "knives"),
    ("leaf", "leaves"),
    ("life", "lives"),
    ("man", "men"),
    ("mouse", "mice"),
    ("ox", "oxen"),
    ("person", "people"),
    ("quiz", "quizzes"),
    ("tooth", "teeth"),
    ("wife", "wives"),
    ("woman", "women"),
];

/// The table of a model named `name`: `Deal` → `deals`, `DealStage` →
/// `deal_stages`, `Company` → `companies`, `Person` → `people`.
pub(crate) fn table_name(name: &str) -> String {
    let snake = name.to_snake_case();
    match snake.rsplit_once('_') {
        Some((head, last)) => format!("{head}_{}", plural(last)),
        None => plural(&snake),
    }
}

/// The plural of an English noun (lower case).
fn plural(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((_, plural)) = IRREGULAR.iter().find(|(singular, _)| *singular == word) {
        return (*plural).to_owned();
    }
    let consonant_before = |suffix_len: usize| {
        word.len() > suffix_len
            && word[..word.len() - suffix_len]
                .chars()
                .last()
                .is_some_and(|c| !"aeiou".contains(c))
    };
    if word.ends_with('y') && consonant_before(1) {
        return format!("{}ies", &word[..word.len() - 1]);
    }
    if word.ends_with("is") && word.len() > 3 {
        return format!("{}es", &word[..word.len() - 2]);
    }
    if ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|suffix| word.ends_with(suffix))
    {
        return format!("{word}es");
    }
    format!("{word}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_snake_case_plurals() {
        for (model, table) in [
            ("Deal", "deals"),
            ("DealStage", "deal_stages"),
            ("Company", "companies"),
            ("Activity", "activities"),
            ("Person", "people"),
            ("Status", "statuses"),
            ("Box", "boxes"),
            ("Branch", "branches"),
            ("Day", "days"),
            ("AuditLog", "audit_logs"),
            ("Metadata", "metadata"),
            ("Analysis", "analyses"),
            ("ApiKey", "api_keys"),
        ] {
            assert_eq!(table_name(model), table, "{model}");
        }
    }
}
