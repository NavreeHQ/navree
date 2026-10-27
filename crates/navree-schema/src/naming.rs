//! Conventional names: indexes (`{table}_{columns}_{kind}`) and the table a
//! foreign key column points at (`company_id` → `companies`).

/// The longest identifier generated names may have: PostgreSQL's limit (63
/// bytes); MySQL allows 64. Longer names are shortened the same way on every
/// database, so dropping by columns finds them.
pub(crate) const MAX_IDENTIFIER: usize = 63;

/// `{table}_{columns}_{suffix}`, lower-cased, with `-` and `.` replaced by
/// `_`, shortened to [`MAX_IDENTIFIER`] bytes.
pub(crate) fn index_name(table: &str, columns: &[String], suffix: &str) -> String {
    let mut name = table.to_owned();
    for column in columns {
        name.push('_');
        name.push_str(column);
    }
    name.push('_');
    name.push_str(suffix);
    shorten(&name.to_lowercase().replace(['-', '.'], "_"))
}

/// `name` itself when it fits; otherwise its first 54 bytes (on a character
/// boundary), `_`, and 8 hex digits of its FNV-1a hash.
pub(crate) fn shorten(name: &str) -> String {
    if name.len() <= MAX_IDENTIFIER {
        return name.to_owned();
    }
    let mut cut = MAX_IDENTIFIER - 9;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}_{:08x}", &name[..cut], fnv1a(name))
}

/// The 32-bit FNV-1a hash of `text`.
fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The table a foreign key column refers to by convention: the part before
/// `_{column}`, pluralized (`company_id` → `companies`, `blog_post_id` →
/// `blog_posts`, `author` → `authors`).
pub(crate) fn table_for_foreign_key(column_name: &str, referenced: &str) -> String {
    let suffix = format!("_{referenced}");
    let base = column_name
        .strip_suffix(&suffix)
        .filter(|base| !base.is_empty())
        .unwrap_or(column_name);
    plural(base)
}

/// The plural of the last `_`-separated word of `snake` (English rules, best
/// effort; a plural stays as it is).
pub(crate) fn plural(snake: &str) -> String {
    match snake.rfind('_') {
        Some(at) => format!("{}_{}", &snake[..at], plural_word(&snake[at + 1..])),
        None => plural_word(snake),
    }
}

const IRREGULAR: &[(&str, &str)] = &[
    ("person", "people"),
    ("child", "children"),
    ("man", "men"),
    ("woman", "women"),
    ("mouse", "mice"),
    ("goose", "geese"),
    ("tooth", "teeth"),
    ("foot", "feet"),
    ("status", "statuses"),
    ("bus", "buses"),
    ("campus", "campuses"),
    ("virus", "viruses"),
    ("cache", "caches"),
    ("niche", "niches"),
    ("movie", "movies"),
    ("leaf", "leaves"),
    ("life", "lives"),
    ("knife", "knives"),
    ("wife", "wives"),
    ("half", "halves"),
    ("analysis", "analyses"),
    ("crisis", "crises"),
    ("criterion", "criteria"),
];

const UNCOUNTABLE: &[&str] = &[
    "data",
    "equipment",
    "feedback",
    "fish",
    "information",
    "media",
    "metadata",
    "money",
    "news",
    "series",
    "sheep",
    "species",
    "staff",
];

fn singular_word(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((singular, _)) = IRREGULAR.iter().find(|(s, p)| *p == word || *s == word) {
        return (*singular).to_owned();
    }
    let strip = |n: usize| word[..word.len() - n].to_owned();
    if word.len() > 3 && word.ends_with("ies") {
        format!("{}y", strip(3))
    } else if word.ends_with("sses")
        || word.ends_with("xes")
        || word.ends_with("zzes")
        || word.ends_with("ches")
        || word.ends_with("shes")
    {
        strip(2)
    } else if word.ends_with("ss") || word.ends_with("us") || word.ends_with("is") {
        word.to_owned()
    } else if word.len() > 1 && word.ends_with('s') {
        strip(1)
    } else {
        word.to_owned()
    }
}

fn plural_word(word: &str) -> String {
    if UNCOUNTABLE.contains(&word) {
        return word.to_owned();
    }
    if let Some((_, plural)) = IRREGULAR.iter().find(|(s, p)| *s == word || *p == word) {
        return (*plural).to_owned();
    }
    if singular_word(word) != word {
        return word.to_owned();
    }
    let consonant_y = word.len() > 1
        && word.ends_with('y')
        && !matches!(
            word.as_bytes()[word.len() - 2],
            b'a' | b'e' | b'i' | b'o' | b'u'
        );
    if consonant_y {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_names_follow_the_convention() {
        let columns = |c: &[&str]| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            index_name("deals", &columns(&["company_id"]), "foreign"),
            "deals_company_id_foreign"
        );
        assert_eq!(
            index_name("Analytics.Events", &columns(&["occurred-at"]), "index"),
            "analytics_events_occurred_at_index"
        );
    }

    #[test]
    fn long_names_are_shortened_with_a_hash() {
        let columns = vec![
            "a_really_long_column_name".to_owned(),
            "another_really_long_column_name".to_owned(),
        ];
        let name = index_name("workspace_member_invitations", &columns, "unique");
        assert_eq!(name.len(), MAX_IDENTIFIER);
        assert!(name.starts_with("workspace_member_invitations_a_really_long_column_name"));
        assert_eq!(
            name,
            index_name("workspace_member_invitations", &columns, "unique"),
            "stable"
        );
        assert_ne!(
            name,
            index_name("workspace_member_invitations", &columns, "index")
        );
    }

    #[test]
    fn foreign_key_tables_are_pluralized() {
        for (column, table) in [
            ("company_id", "companies"),
            ("user_id", "users"),
            ("blog_post_id", "blog_posts"),
            ("person_id", "people"),
            ("status_id", "statuses"),
            ("box_id", "boxes"),
            ("owner", "owners"),
            ("media_id", "media"),
        ] {
            assert_eq!(table_for_foreign_key(column, "id"), table, "{column}");
        }
        assert_eq!(table_for_foreign_key("country_code", "code"), "countries");
    }
}
