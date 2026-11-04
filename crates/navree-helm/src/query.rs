//! [`ListQuery`]: a list's search, filters, sort and page, read from the URL
//! (`?search=..&filter[stage][in]=lead,won&sort=-amount_cents&page=2`).
//!
//! The names are the ones every Navree list uses (see `navree_http::list`):
//! `filter[name]=a` and `filter[name][in]=a,b`, `sort`, `search`, `page` and
//! `per_page`. `via[column]` and `range` are Helm's own and are read here too.

use std::collections::BTreeMap;

use navree_db::MAX_PER_PAGE;
use navree_http::AppError;
use navree_http::list::{self as shared, Condition, Op, SortKey, encode_component};

/// What a list shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ListQuery {
    pub(crate) search: Option<String>,
    /// `filter[name]=a` or `filter[name][in]=a,b` (the key may repeat).
    pub(crate) filters: BTreeMap<String, Vec<String>>,
    pub(crate) sort: Option<String>,
    pub(crate) page: u64,
    pub(crate) per_page: Option<u64>,
    /// `via[column]=id`: the rows whose `column` (a `belongs_to` field)
    /// holds `id`, for a `has_many` panel.
    pub(crate) via: Option<(String, String)>,
    /// The metric range of the list's cards.
    pub(crate) range: Option<String>,
    /// The list page counts its rows only up to the resource's `COUNT_CAP`
    /// (set by the page, never by the URL; the API counts them all).
    pub(crate) capped: bool,
}

/// How strictly a query string is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// A screen (the list page, its export): a bad address still shows a
    /// page. What Helm does not read is ignored; a bad `page` or `per_page`
    /// is a 400.
    Screen,
    /// The JSON API: anything the list query does not allow is a 400 that
    /// names the parameter.
    Api,
}

impl ListQuery {
    /// Parses the query string of a screen; a 400 naming the parameter for a
    /// bad page or page size. A parameter Helm does not read (an operator
    /// other than equal and `in`, a cursor) is left out, and so is one that is
    /// not the list query's (`filters[name]`, `q`).
    pub(crate) fn parse(query: &str) -> Result<Self, AppError> {
        Self::read(query, Mode::Screen)
    }

    /// Parses the query string of the JSON API: as [`parse`](Self::parse), and
    /// every parameter the list query does not allow is a 400 naming it.
    pub(crate) fn parse_api(query: &str) -> Result<Self, AppError> {
        Self::read(query, Mode::Api)
    }

    fn read(query: &str, mode: Mode) -> Result<Self, AppError> {
        let (syntax, problems) = shared::ListQuery::parse_lenient(query);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for problem in problems {
            let number = matches!(problem.param.as_str(), "page" | "per_page");
            if !number && mode == Mode::Screen {
                continue;
            }
            let message = if problem.param == "per_page" {
                format!("per_page must be between 1 and {MAX_PER_PAGE}")
            } else {
                problem.message
            };
            refuse(&mut errors, problem.param, message);
        }
        if syntax.per_page.is_some_and(|n| n > MAX_PER_PAGE) {
            refuse(
                &mut errors,
                "per_page",
                format!("per_page must be between 1 and {MAX_PER_PAGE}"),
            );
        }
        if mode == Mode::Api {
            for (param, given) in [
                ("cursor", syntax.cursor.is_some()),
                ("before", syntax.before.is_some()),
                ("limit", syntax.limit.is_some()),
            ] {
                if given {
                    refuse(
                        &mut errors,
                        param,
                        "a Helm list pages with page and per_page",
                    );
                }
            }
        }
        let mut filters: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for condition in &syntax.filter {
            if matches!(condition.op, Op::Eq | Op::In) {
                filters
                    .entry(condition.field.clone())
                    .or_default()
                    .extend(
                        condition
                            .values
                            .iter()
                            .map(|value| value.trim())
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                    );
            } else if mode == Mode::Api {
                refuse(
                    &mut errors,
                    condition.param(),
                    format!(
                        "a Helm filter is equal to a value (filter[{field}]=a) or one of a list (filter[{field}][in]=a,b), not `{op}`",
                        field = condition.field,
                        op = condition.op,
                    ),
                );
            }
        }
        filters.retain(|_, values| !values.is_empty());
        if !errors.is_empty() {
            return Err(AppError::validation(errors));
        }
        let (via, range) = own_parameters(query);
        Ok(ListQuery {
            search: syntax.search,
            filters,
            // Helm sorts by one field: the first term of the shared syntax.
            sort: syntax.sort.first().map(SortKey::term),
            page: syntax.page.unwrap_or(1),
            per_page: syntax.per_page,
            via,
            range,
            capped: false,
        })
    }

    /// The query string of this list (for links), with `page` replaced (page
    /// 1 is left out) and, when given, `sort`. Filters, sort, search, page and
    /// page size are written as every Navree list writes them, then Helm's own
    /// `via` and `range`.
    pub(crate) fn to_query(&self, page: Option<u64>, sort: Option<&str>) -> String {
        let mut list = shared::ListQuery::new();
        for (name, values) in &self.filters {
            if values.is_empty() {
                continue;
            }
            list.filter.push(Condition {
                field: name.clone(),
                op: if values.len() == 1 { Op::Eq } else { Op::In },
                values: values.clone(),
            });
        }
        if let Some(term) = sort.or(self.sort.as_deref()) {
            list.sort.push(match term.strip_prefix('-') {
                Some(field) => SortKey {
                    field: field.to_owned(),
                    descending: true,
                },
                None => SortKey {
                    field: term.to_owned(),
                    descending: false,
                },
            });
        }
        list.search = self.search.clone();
        list.page = page.filter(|p| *p > 1);
        list.per_page = self.per_page;
        let mut out = list.to_query_string();
        if let Some((column, id)) = &self.via {
            append(&mut out, &format!("via[{column}]"), id);
        }
        if let Some(range) = &self.range {
            append(&mut out, "range", range);
        }
        out
    }
}

/// Notes that `param` is wrong.
fn refuse(
    errors: &mut BTreeMap<String, Vec<String>>,
    param: impl Into<String>,
    message: impl Into<String>,
) {
    errors.entry(param.into()).or_default().push(message.into());
}

/// Adds `name=value` to a query string being written, encoded as the list
/// query encodes its own parameters.
fn append(out: &mut String, name: &str, value: &str) {
    if !out.is_empty() {
        out.push('&');
    }
    out.push_str(&encode_component(name));
    out.push('=');
    out.push_str(&encode_component(value));
}

/// The parameters that are Helm's, not the list query's: `via[column]=id` and
/// `range`. It reads as much of the query as the list query does.
fn own_parameters(query: &str) -> (Option<(String, String)>, Option<String>) {
    let query = query.strip_prefix('?').unwrap_or(query);
    let bytes = &query.as_bytes()[..query.len().min(shared::MAX_QUERY_BYTES)];
    let mut via = None;
    let mut range = None;
    for (key, value) in form_urlencoded::parse(bytes).take(shared::MAX_PARAMS) {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if key == "range" {
            range = Some(value.to_owned());
        } else if let Some(column) = bracketed(&key, "via") {
            via = Some((column.to_owned(), value.to_owned()));
        }
    }
    (via, range)
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

    fn keys(error: AppError) -> Vec<String> {
        assert_eq!(error.status.as_u16(), 400);
        error.errors.unwrap().keys().cloned().collect()
    }

    #[test]
    fn parses_search_filters_sort_and_page() {
        let q = ListQuery::parse(
            "search=Globex&filter%5Bstage%5D%5Bin%5D=lead&filter[stage][in]=won,lost&filter[open]=1&sort=-amount_cents&page=2&per_page=50&via[company_id]=abc&range=7",
        )
        .unwrap();
        assert_eq!(q.search.as_deref(), Some("Globex"));
        assert_eq!(q.filters["stage"], ["lead", "won", "lost"]);
        assert_eq!(q.filters["open"], ["1"]);
        assert_eq!(q.sort.as_deref(), Some("-amount_cents"));
        assert_eq!(q.page, 2);
        assert_eq!(q.per_page, Some(50));
        assert_eq!(q.via, Some(("company_id".into(), "abc".into())));
        assert_eq!(q.range.as_deref(), Some("7"));
        assert!(!q.capped);
    }

    #[test]
    fn an_equal_filter_is_one_value_and_in_is_a_list() {
        let q = ListQuery::parse("filter[stage]=won").unwrap();
        assert_eq!(q.filters["stage"], ["won"]);
        let q = ListQuery::parse("filter[stage][eq]=won").unwrap();
        assert_eq!(q.filters["stage"], ["won"]);
        let q = ListQuery::parse("filter[stage][in]=lead,won").unwrap();
        assert_eq!(q.filters["stage"], ["lead", "won"]);
        // The key may repeat, and a list of one is a list.
        let q = ListQuery::parse("filter[stage][in]=lead&filter[stage][in]=won").unwrap();
        assert_eq!(q.filters["stage"], ["lead", "won"]);
        let q = ListQuery::parse("filter[stage][in]=won").unwrap();
        assert_eq!(q.filters["stage"], ["won"]);
        // An equal filter keeps its comma: it is text a custom filter reads.
        let q = ListQuery::parse("filter[city]=Ludhiana,Punjab").unwrap();
        assert_eq!(q.filters["city"], ["Ludhiana,Punjab"]);
    }

    #[test]
    fn names_outside_the_list_query_are_left_alone() {
        let q = ListQuery::parse(
            "filters[stage]=won&filters%5Bstage%5D=won&filters[stage][]=won&q=acme",
        )
        .unwrap();
        assert_eq!(
            q,
            ListQuery {
                page: 1,
                ..ListQuery::default()
            }
        );
        // The JSON API leaves them alone too: they are handler parameters.
        let q = ListQuery::parse_api("filters[stage]=won&q=acme").unwrap();
        assert!(q.filters.is_empty());
        assert_eq!(q.search, None);
    }

    #[test]
    fn q_is_not_a_search() {
        assert_eq!(ListQuery::parse("q=acme").unwrap().search, None);
        let q = ListQuery::parse("q=acme&search=globex").unwrap();
        assert_eq!(q.search.as_deref(), Some("globex"));
    }

    #[test]
    fn sort_is_one_field_the_first_term() {
        let q = ListQuery::parse("sort=-amount_cents,title").unwrap();
        assert_eq!(q.sort.as_deref(), Some("-amount_cents"));
        let q = ListQuery::parse("sort=title&sort=-amount_cents").unwrap();
        assert_eq!(q.sort.as_deref(), Some("title"));
        let q = ListQuery::parse("sort=").unwrap();
        assert_eq!(q.sort, None);
        // A page leaves out a sort that is not a name; the API names it.
        let q = ListQuery::parse("sort=-").unwrap();
        assert_eq!(q.sort, None);
        assert_eq!(keys(ListQuery::parse_api("sort=-").unwrap_err()), ["sort"]);
    }

    #[test]
    fn empty_values_are_ignored_and_bad_pages_rejected() {
        let q = ListQuery::parse("search=&filter[stage]=&filter[stage][in]=&page=").unwrap();
        assert_eq!(
            q,
            ListQuery {
                page: 1,
                ..ListQuery::default()
            }
        );
        let error = ListQuery::parse("page=0").unwrap_err();
        assert_eq!(keys(error), ["page"]);
        assert_eq!(keys(ListQuery::parse("page=abc").unwrap_err()), ["page"]);
        let error = ListQuery::parse("per_page=1000").unwrap_err();
        assert!(error.errors.unwrap().contains_key("per_page"));
        assert_eq!(keys(ListQuery::parse("per_page=0").unwrap_err()), ["per_page"]);
        assert_eq!(keys(ListQuery::parse_api("per_page=101").unwrap_err()), ["per_page"]);
        assert_eq!(ListQuery::parse("per_page=100").unwrap().per_page, Some(100));
    }

    #[test]
    fn a_screen_leaves_out_what_helm_does_not_read() {
        let q = ListQuery::parse(
            "filter[stage]=won&filter[stage][regex]=x&filter[amount][gte]=5&filter[owner][null]=true&filter[city][contains]=a&filter[x][ne]=1&cursor=abc&limit=5",
        )
        .unwrap();
        assert_eq!(q.filters.len(), 1);
        assert_eq!(q.filters["stage"], ["won"]);
        assert_eq!(q.page, 1);
        assert_eq!(q.per_page, None);
    }

    #[test]
    fn the_api_names_every_parameter_it_does_not_allow() {
        // Not an operator at all.
        assert_eq!(
            keys(ListQuery::parse_api("filter[stage][regex]=x").unwrap_err()),
            ["filter[stage][regex]"]
        );
        // An operator the list query has and Helm filters do not take.
        for (query, param) in [
            ("filter[amount][gte]=5", "filter[amount][gte]"),
            ("filter[amount][lt]=5", "filter[amount][lt]"),
            ("filter[city][contains]=a", "filter[city][contains]"),
            ("filter[city][starts_with]=a", "filter[city][starts_with]"),
            ("filter[stage][ne]=won", "filter[stage][ne]"),
            ("filter[stage][not_in]=won,lost", "filter[stage][not_in]"),
            ("filter[owner_id][null]=true", "filter[owner_id][null]"),
        ] {
            let errors = ListQuery::parse_api(query).unwrap_err().errors.unwrap();
            assert_eq!(errors.keys().collect::<Vec<_>>(), [param], "{query}");
            // It says what a Helm filter takes.
            assert!(errors[param][0].contains("[in]"), "{:?}", errors[param]);
        }
        // A filter that is not written as one, a second equal filter, a name
        // that is not a field, a cursor, mixed paging.
        for (query, param) in [
            ("filter=won", "filter"),
            ("filter[stage]=won&filter[stage]=lost", "filter[stage]"),
            ("filter[st age]=won", "filter[st age]"),
            ("cursor=abc", "cursor"),
            ("before=abc", "before"),
            ("limit=10", "limit"),
            ("page=2&limit=10", "limit"),
        ] {
            let error = ListQuery::parse_api(query).unwrap_err();
            assert!(error.errors.unwrap().contains_key(param), "{query}");
        }
        // The same query, written for a page, reads what it can.
        let q = ListQuery::parse("filter[stage]=won&filter[stage]=lost&cursor=abc").unwrap();
        assert_eq!(q.filters["stage"], ["won"]);
    }

    #[test]
    fn the_api_reads_the_rest_of_the_grammar() {
        let q = ListQuery::parse_api(
            "filter[stage][in]=a,b&filter[open]=1&sort=-amount_cents&search=x&page=2&per_page=100&via[company_id]=abc&range=30&owner_id=3&utm_source=mail",
        )
        .unwrap();
        assert_eq!(q.filters["stage"], ["a", "b"]);
        assert_eq!(q.page, 2);
        assert_eq!(q.per_page, Some(100));
        assert_eq!(q.via, Some(("company_id".into(), "abc".into())));
    }

    #[test]
    fn round_trips_into_links() {
        let q = ListQuery::parse("search=a b&filter[stage]=won&sort=title").unwrap();
        assert_eq!(
            q.to_query(Some(3), None),
            "filter%5Bstage%5D=won&sort=title&search=a+b&page=3"
        );
        assert_eq!(
            q.to_query(None, Some("-title")),
            "filter%5Bstage%5D=won&sort=-title&search=a+b"
        );
        assert_eq!(q.to_query(Some(1), None), "filter%5Bstage%5D=won&sort=title&search=a+b");
    }

    #[test]
    fn a_list_of_values_is_written_as_in_and_one_value_as_equal() {
        let q = ListQuery::parse("filter[stage][in]=won,lost&filter[open][in]=1").unwrap();
        assert_eq!(
            q.to_query(None, None),
            "filter%5Bopen%5D=1&filter%5Bstage%5D%5Bin%5D=won%2Clost"
        );
        // Only the list query's names are written.
        let text = q.to_query(Some(2), Some("title"));
        assert!(!text.contains("filters"), "{text}");
        assert!(!text.contains("q="), "{text}");
    }

    #[test]
    fn helms_own_parameters_follow_the_list_query() {
        let q = ListQuery::parse(
            "range=7&via[company_id]=abc&per_page=10&page=2&search=x&filter[stage]=won&sort=-title",
        )
        .unwrap();
        assert_eq!(
            q.to_query(Some(2), None),
            "filter%5Bstage%5D=won&sort=-title&search=x&page=2&per_page=10&via%5Bcompany_id%5D=abc&range=7"
        );
        let mut without = q.clone();
        without.per_page = None;
        assert_eq!(
            without.to_query(None, None),
            "filter%5Bstage%5D=won&sort=-title&search=x&via%5Bcompany_id%5D=abc&range=7"
        );
    }

    #[test]
    fn a_link_reads_back_as_the_list_it_was_made_from() {
        for query in [
            "",
            "search=a+b",
            "search=%E0%A8%AA%E0%A9%B0%E0%A8%9C%E0%A8%BE%E0%A8%AC&page=4",
            "filter[stage]=won",
            "filter[stage][in]=lead,won,lost&filter[open]=1&sort=-amount_cents",
            "filter[expected_close_date]=2026-10-01..2026-10-31&per_page=50&page=3",
            "filter[city]=Ludhiana,Punjab&search=x",
            "via[company_id]=abc&range=90&sort=title",
            "sort=-amount_cents,title&filter[company_id][in]=a&filter[company_id][in]=b",
        ] {
            let q = ListQuery::parse(query).unwrap();
            let again = ListQuery::parse(&q.to_query(Some(q.page), None)).unwrap();
            assert_eq!(q, again, "{query}");
            let sorted = ListQuery::parse(&q.to_query(None, q.sort.as_deref())).unwrap();
            assert_eq!(sorted.filters, q.filters, "{query}");
        }
    }
}
