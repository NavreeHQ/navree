//! Resource URI templates: `crm://deals/{id}` (RFC 6570 level 1, one
//! variable per brace pair, matching one non-empty path segment).

use std::collections::BTreeMap;

/// A parsed resource URI or URI template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UriTemplate {
    parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    Variable(String),
}

impl UriTemplate {
    /// Parses `template`; an error explains what is wrong.
    pub(crate) fn parse(template: &str) -> Result<Self, String> {
        if template.trim().is_empty() {
            return Err("a resource URI must not be empty".to_owned());
        }
        if !template.contains(':') {
            return Err(format!(
                "resource URI `{template}` has no scheme: write it like `crm://deals/{{id}}`"
            ));
        }
        let mut parts = Vec::new();
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            if open > 0 {
                parts.push(Part::Literal(rest[..open].to_owned()));
            }
            let close = rest[open..]
                .find('}')
                .map(|i| open + i)
                .ok_or_else(|| format!("resource URI `{template}` has an unclosed `{{`"))?;
            let name = &rest[open + 1..close];
            let valid =
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return Err(format!(
                    "resource URI `{template}`: `{{{name}}}` is not a variable name (letters, digits and `_`)"
                ));
            }
            if matches!(parts.last(), Some(Part::Variable(_))) {
                return Err(format!(
                    "resource URI `{template}`: two variables need text between them"
                ));
            }
            if parts
                .iter()
                .any(|part| matches!(part, Part::Variable(other) if other == name))
            {
                return Err(format!(
                    "resource URI `{template}` names the variable `{name}` twice"
                ));
            }
            parts.push(Part::Variable(name.to_owned()));
            rest = &rest[close + 1..];
        }
        if rest.contains('}') {
            return Err(format!("resource URI `{template}` has an unopened `}}`"));
        }
        if !rest.is_empty() {
            parts.push(Part::Literal(rest.to_owned()));
        }
        Ok(Self { parts })
    }

    /// Whether the template has variables (a resource template rather than
    /// a fixed resource).
    pub(crate) fn is_template(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, Part::Variable(_)))
    }

    /// The variable names, in order.
    pub(crate) fn variables(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Variable(name) => Some(name.as_str()),
                Part::Literal(_) => None,
            })
            .collect()
    }

    /// The variables of `uri` when it matches: each one non-empty, without
    /// `/`, percent-decoded.
    pub(crate) fn matches(&self, uri: &str) -> Option<BTreeMap<String, String>> {
        let mut params = BTreeMap::new();
        let mut rest = uri;
        let mut parts = self.parts.iter().peekable();
        while let Some(part) = parts.next() {
            match part {
                Part::Literal(literal) => rest = rest.strip_prefix(literal.as_str())?,
                Part::Variable(name) => {
                    let end = match parts.peek() {
                        Some(Part::Literal(next)) => rest.find(next.as_str())?,
                        _ => rest.len(),
                    };
                    let value = &rest[..end];
                    if value.is_empty() || value.contains('/') {
                        return None;
                    }
                    params.insert(name.clone(), percent_decode(value)?);
                    rest = &rest[end..];
                }
            }
        }
        rest.is_empty().then_some(params)
    }
}

/// Decodes `%XX` escapes; `None` for invalid escapes or UTF-8.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Option<BTreeMap<String, String>> {
        Some(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn fixed_uris_match_themselves() {
        let uri = UriTemplate::parse("crm://pipeline").unwrap();
        assert!(!uri.is_template());
        assert_eq!(uri.matches("crm://pipeline"), params(&[]));
        assert_eq!(uri.matches("crm://pipeline/x"), None);
        assert_eq!(uri.matches("crm://pipelines"), None);
    }

    #[test]
    fn templates_capture_one_segment_per_variable() {
        let uri = UriTemplate::parse("crm://companies/{company}/deals/{id}").unwrap();
        assert!(uri.is_template());
        assert_eq!(uri.variables(), ["company", "id"]);
        assert_eq!(
            uri.matches("crm://companies/agra/deals/0199"),
            params(&[("company", "agra"), ("id", "0199")])
        );
        assert_eq!(uri.matches("crm://companies//deals/1"), None);
        assert_eq!(uri.matches("crm://companies/a/b/deals/1"), None);
        assert_eq!(uri.matches("crm://companies/a/deals/1/x"), None);

        let file = UriTemplate::parse("docs://{name}.md").unwrap();
        assert_eq!(
            file.matches("docs://read%20me.md"),
            params(&[("name", "read me")])
        );
        assert_eq!(file.matches("docs://bad%zz.md"), None);
    }

    #[test]
    fn invalid_templates_are_explained() {
        for (template, fragment) in [
            ("", "empty"),
            ("deals/{id}", "no scheme"),
            ("crm://deals/{id", "unclosed"),
            ("crm://deals/{}", "variable name"),
            ("crm://deals/{a-b}", "variable name"),
            ("crm://deals/{a}{b}", "between them"),
            ("crm://{id}/x/{id}", "twice"),
            ("crm://deals/id}", "unopened"),
        ] {
            let error = UriTemplate::parse(template).unwrap_err();
            assert!(error.contains(fragment), "{template}: {error}");
        }
    }
}
