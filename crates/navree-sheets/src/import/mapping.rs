//! [`Mapping`]: which column of a file each field reads, found
//! automatically or saved from an earlier import.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use super::ImportColumn;

/// Which header of the file each field reads: `{"email": "E-mail address",
/// "first_name": "First name"}`. Fields that are not in the mapping are
/// left empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(transparent)]
pub struct Mapping(BTreeMap<String, String>);

impl Mapping {
    /// An empty mapping.
    pub fn new() -> Self {
        Mapping::default()
    }

    /// Maps `field` to the file's column `header`.
    pub fn map(mut self, field: impl Into<String>, header: impl Into<String>) -> Self {
        self.0.insert(field.into(), header.into());
        self
    }

    /// The header `field` reads.
    pub fn header(&self, field: &str) -> Option<&str> {
        self.0.get(field).map(String::as_str)
    }

    /// Every field and its header.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(f, h)| (f.as_str(), h.as_str()))
    }

    /// The number of mapped fields.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no field is mapped.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<F: Into<String>, H: Into<String>> FromIterator<(F, H)> for Mapping {
    fn from_iter<T: IntoIterator<Item = (F, H)>>(iter: T) -> Self {
        Mapping(
            iter.into_iter()
                .map(|(f, h)| (f.into(), h.into()))
                .collect(),
        )
    }
}

/// A header reduced to what matching compares: lower case, letters and
/// digits only (`E-mail Address` and `email_address` match).
pub(crate) fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The mapping found from the headers: each column takes the first unused
/// header that matches its field name, its header or one of its aliases
/// (compared by [`normalize`]).
pub(crate) fn automatic(columns: &[ImportColumn], headers: &[String]) -> Mapping {
    let normalized: Vec<String> = headers.iter().map(|h| normalize(h)).collect();
    let mut used = BTreeSet::new();
    let mut mapping = Mapping::new();
    for column in columns {
        let names: Vec<String> = std::iter::once(column.field.as_ref())
            .chain(std::iter::once(column.header.as_ref()))
            .chain(column.aliases.iter().map(AsRef::as_ref))
            .map(normalize)
            .filter(|n| !n.is_empty())
            .collect();
        // An exact header wins over a normalized match.
        let exact = headers
            .iter()
            .position(|h| h == column.header.as_ref() && !used.contains(h));
        let found = exact.or_else(|| {
            normalized
                .iter()
                .enumerate()
                .find(|(i, n)| names.contains(n) && !used.contains(&headers[*i]))
                .map(|(i, _)| i)
        });
        if let Some(index) = found {
            used.insert(headers[index].clone());
            mapping
                .0
                .insert(column.field.to_string(), headers[index].clone());
        }
    }
    mapping
}

/// The fields of required columns that `mapping` leaves out.
pub(crate) fn missing(mapping: &Mapping, columns: &[ImportColumn]) -> Vec<String> {
    columns
        .iter()
        .filter(|c| c.required && mapping.header(&c.field).is_none())
        .map(|c| c.field.to_string())
        .collect()
}

/// Checks a mapping someone chose: every field is a column of the
/// importer, every header is in the file, no header is used twice and
/// every required column is mapped. Errors are keyed by field.
pub(crate) fn check(
    mapping: &Mapping,
    columns: &[ImportColumn],
    headers: &[String],
) -> Result<(), BTreeMap<String, Vec<String>>> {
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for (field, header) in mapping.iter() {
        if !columns.iter().any(|c| c.field == field) {
            errors
                .entry(field.to_owned())
                .or_default()
                .push(format!("{field} is not a column of this import"));
            continue;
        }
        if !headers.iter().any(|h| h == header) {
            errors.entry(field.to_owned()).or_default().push(format!(
                "{field} is mapped to `{header}`, which is not a column of the file"
            ));
            continue;
        }
        if let Some(other) = seen.insert(header, field) {
            errors.entry(field.to_owned()).or_default().push(format!(
                "{field} reads `{header}`, which {other} already reads"
            ));
        }
    }
    // A required field mapped to a header the file lacks is unmapped too.
    for column in columns.iter().filter(|c| c.required) {
        let mapped = mapping
            .header(&column.field)
            .is_some_and(|header| headers.iter().any(|h| h == header));
        if !mapped {
            errors
                .entry(column.field.to_string())
                .or_default()
                .push(format!("{} should not be empty", column.field));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The fingerprint of a header row: saved mappings are found by it, so a
/// file with the same columns (in any order, any case) reuses one.
pub(crate) fn fingerprint(headers: &[String]) -> String {
    let mut names: Vec<String> = headers.iter().map(|h| normalize(h)).collect();
    names.sort();
    let mut hasher = Sha256::new();
    for name in names {
        hasher.update(name.as_bytes());
        hasher.update([0x1f]);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<ImportColumn> {
        vec![
            ImportColumn::new("email", "Email")
                .alias("E-mail address")
                .required(true),
            ImportColumn::new("first_name", "First name").required(false),
            ImportColumn::new("company", "Company").required(true),
        ]
    }

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn headers_are_matched_by_field_header_or_alias() {
        let found = automatic(
            &columns(),
            &headers(&["FIRST_NAME", "e-mail Address", "Notes"]),
        );
        assert_eq!(found.header("email"), Some("e-mail Address"));
        assert_eq!(found.header("first_name"), Some("FIRST_NAME"));
        assert_eq!(found.header("company"), None);
        assert_eq!(missing(&found, &columns()), ["company"]);
    }

    #[test]
    fn a_header_is_used_once() {
        let columns = vec![
            ImportColumn::new("email", "Email"),
            ImportColumn::new("work_email", "Work email").alias("Email"),
        ];
        let found = automatic(&columns, &headers(&["Email"]));
        assert_eq!(found.header("email"), Some("Email"));
        assert_eq!(found.header("work_email"), None);
    }

    #[test]
    fn a_chosen_mapping_is_checked() {
        let file = headers(&["Mail", "Org", "Name"]);
        let good = Mapping::new().map("email", "Mail").map("company", "Org");
        assert!(check(&good, &columns(), &file).is_ok());

        let bad = Mapping::new()
            .map("email", "Missing")
            .map("company", "Org")
            .map("first_name", "Org")
            .map("nickname", "Name");
        let errors = check(&bad, &columns(), &file).unwrap_err();
        assert_eq!(
            errors["email"],
            [
                "email is mapped to `Missing`, which is not a column of the file",
                "email should not be empty"
            ]
        );
        assert_eq!(
            errors["first_name"],
            ["first_name reads `Org`, which company already reads"]
        );
        assert_eq!(
            errors["nickname"],
            ["nickname is not a column of this import"]
        );
    }

    #[test]
    fn fingerprints_ignore_order_and_case() {
        let a = fingerprint(&headers(&["Email", "First name"]));
        let b = fingerprint(&headers(&["first_name", "EMAIL"]));
        let c = fingerprint(&headers(&["Email"]));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn arabic_headers_normalize() {
        assert_eq!(normalize("البريد الإلكتروني"), "البريدالإلكتروني");
        assert_eq!(normalize("E-Mail_Address"), "emailaddress");
    }
}
