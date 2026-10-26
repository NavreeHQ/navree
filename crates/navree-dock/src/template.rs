//! `{placeholder}` substitution in the values of a service's `env` table.

/// The placeholder names in `template`: every `{name}` whose name is made of
/// lower-case letters, digits and `_`. Other braces are left alone.
#[cfg(test)]
pub(crate) fn placeholders_in(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    scan(template, |name| {
        names.push(name.to_owned());
        None
    });
    names
}

/// `template` with each `{name}` replaced by `lookup(name)`; the first name
/// `lookup` does not know is the error.
pub(crate) fn render(
    template: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let mut unknown = None;
    let rendered = scan(template, |name| {
        let value = lookup(name);
        if value.is_none() && unknown.is_none() {
            unknown = Some(name.to_owned());
        }
        value
    });
    match unknown {
        Some(name) => Err(name),
        None => Ok(rendered),
    }
}

/// Walks `template`, calling `visit` for each placeholder and substituting
/// what it returns (or keeping the placeholder when it returns `None`).
fn scan(template: &str, mut visit: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(after.len());
        if name_len > 0 && after[name_len..].starts_with('}') {
            let name = &after[..name_len];
            match visit(name) {
                Some(value) => out.push_str(&value),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            }
            rest = &after[name_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "host" => Some("127.0.0.1".into()),
            "port" => Some("5433".into()),
            _ => None,
        }
    }

    #[test]
    fn placeholders_are_substituted() {
        assert_eq!(
            render("postgres://{host}:{port}/x?app=crm-dev", lookup).unwrap(),
            "postgres://127.0.0.1:5433/x?app=crm-dev"
        );
        assert_eq!(render("plain", lookup).unwrap(), "plain");
    }

    #[test]
    fn other_braces_stay_and_unknown_names_are_errors() {
        assert_eq!(
            render(r#"{"a": 1} {Host} {} {"#, lookup).unwrap(),
            r#"{"a": 1} {Host} {} {"#
        );
        assert_eq!(render("{host}:{bucket}", lookup).unwrap_err(), "bucket");
        assert_eq!(
            placeholders_in("{user}:{password}@{host} {X}"),
            ["user", "password", "host"]
        );
    }
}
