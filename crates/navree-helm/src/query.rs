//! [`ListQuery`]: a list's search, filters, sort and page, read from the
//! URL (`?search=..&filters[stage]=won&sort=-amount_cents&page=2`).

use std::collections::BTreeMap;

use navree_http::AppError;

/// What a list shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ListQuery {
    pub(crate) search: Option<String>,
    /// `filters[name]=a&filters[name]=b` or `filters[name]=a,b`.
    pub(crate) filters: BTreeMap<String, Vec<String>>,
    pub(crate) sort: Option<String>,
    pub(crate) page: u64,
    pub(crate) per_page: Option<u64>,
    /// `via[column]=id`: the rows whose `column` (a `belongs_to` field)
    /// holds `id`, for a `has_many` panel.
    pub(crate) via: Option<(String, String)>,
    /// The metric range of the list's cards.
    pub(crate) range: Option<String>,
}

impl ListQuery {
    /// Parses a query string; a 400 naming the parameter for a bad page or
    /// page size.
    pub(crate) fn parse(query: &str) -> Result<Self, AppError> {
        let mut list = ListQuery {
            page: 1,
            ..ListQuery::default()
        };
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            let value = value.trim().to_owned();
            match key.as_ref() {
                "search" | "q" => {
                    if !value.is_empty() {
                        list.search = Some(value);
                    }
                }
                "sort" => {
                    if !value.is_empty() {
                        list.sort = Some(value);
                    }
                }
                "range" => {
                    if !value.is_empty() {
                        list.range = Some(value);
                    }
                }
                "page" => match value.parse::<u64>() {
                    Ok(page) if page >= 1 => list.page = page,
                    _ if value.is_empty() => {}
                    _ => {
                        errors.insert("page".into(), vec!["page must not be less than 1".into()]);
                    }
                },
                "per_page" => match value.parse::<u64>() {
                    Ok(n) if (1..=navree_db::MAX_PER_PAGE).contains(&n) => {
                        list.per_page = Some(n);
                    }
                    _ if value.is_empty() => {}
                    _ => {
                        errors.insert(
                            "per_page".into(),
                            vec![format!(
                                "per_page must be between 1 and {}",
                                navree_db::MAX_PER_PAGE
                            )],
                        );
                    }
                },
                other => {
                    if let Some(name) = bracketed(other, "filters") {
                        if value.is_empty() {
                            continue;
                        }
                        let values = list.filters.entry(name.to_owned()).or_default();
                        values.extend(
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(str::to_owned),
                        );
                    } else if let Some(column) = bracketed(other, "via")
                        && !value.is_empty()
                    {
                        list.via = Some((column.to_owned(), value));
                    }
                }
            }
        }
        list.filters.retain(|_, values| !values.is_empty());
        if errors.is_empty() {
            Ok(list)
        } else {
            Err(AppError::validation(errors))
        }
    }

    /// The query string of this list (for links), with `page` replaced.
    pub(crate) fn to_query(&self, page: Option<u64>, sort: Option<&str>) -> String {
        let mut out = form_urlencoded::Serializer::new(String::new());
        if let Some(search) = &self.search {
            out.append_pair("search", search);
        }
        for (name, values) in &self.filters {
            for value in values {
                out.append_pair(&format!("filters[{name}]"), value);
            }
        }
        if let Some((column, id)) = &self.via {
            out.append_pair(&format!("via[{column}]"), id);
        }
        match (sort, &self.sort) {
            (Some(sort), _) => {
                out.append_pair("sort", sort);
            }
            (None, Some(sort)) => {
                out.append_pair("sort", sort);
            }
            _ => {}
        }
        if let Some(per_page) = self.per_page {
            out.append_pair("per_page", &per_page.to_string());
        }
        if let Some(range) = &self.range {
            out.append_pair("range", range);
        }
        if let Some(page) = page.filter(|p| *p > 1) {
            out.append_pair("page", &page.to_string());
        }
        out.finish()
    }
}

/// `name` for `prefix[name]` or `prefix[name][]`.
fn bracketed<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = key.strip_prefix(prefix)?.strip_prefix('[')?;
    let rest = rest.strip_suffix("[]").unwrap_or(rest);
    let name = rest.strip_suffix(']')?;
    (!name.is_empty() && !name.contains(['[', ']'])).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_filters_sort_and_page() {
        let q = ListQuery::parse(
            "search=Globex&filters%5Bstage%5D=lead&filters[stage]=won,lost&filters[open]=1&sort=-amount_cents&page=2&per_page=50&via[company_id]=abc",
        )
        .unwrap();
        assert_eq!(q.search.as_deref(), Some("Globex"));
        assert_eq!(q.filters["stage"], ["lead", "won", "lost"]);
        assert_eq!(q.filters["open"], ["1"]);
        assert_eq!(q.sort.as_deref(), Some("-amount_cents"));
        assert_eq!(q.page, 2);
        assert_eq!(q.per_page, Some(50));
        assert_eq!(q.via, Some(("company_id".into(), "abc".into())));
    }

    #[test]
    fn empty_values_are_ignored_and_bad_pages_rejected() {
        let q = ListQuery::parse("search=&filters[stage]=&page=").unwrap();
        assert_eq!(
            q,
            ListQuery {
                page: 1,
                ..ListQuery::default()
            }
        );
        let error = ListQuery::parse("page=0").unwrap_err();
        assert_eq!(error.status.as_u16(), 400);
        let error = ListQuery::parse("per_page=1000").unwrap_err();
        assert!(error.errors.unwrap().contains_key("per_page"));
    }

    #[test]
    fn round_trips_into_links() {
        let q = ListQuery::parse("search=a b&filters[stage]=won&sort=title").unwrap();
        assert_eq!(
            q.to_query(Some(3), None),
            "search=a+b&filters%5Bstage%5D=won&sort=title&page=3"
        );
        assert_eq!(
            q.to_query(None, Some("-title")),
            "search=a+b&filters%5Bstage%5D=won&sort=-title"
        );
    }
}
