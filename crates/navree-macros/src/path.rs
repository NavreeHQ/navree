//! Route path templates (navree-http's syntax: `{param}`, `{*rest}`, each a
//! whole segment), checked at compile time so mistakes navree-http would
//! reject at boot fail the build.

use syn::LitStr;

/// Checks the template in `path`: no old-style `:param` / `*rest` captures,
/// balanced braces, and parameters that are whole segments. The error
/// points at the literal.
pub(crate) fn check(path: &LitStr) -> syn::Result<()> {
    let value = path.value();
    for segment in value.trim_start_matches('!').split('/') {
        let problem = if let Some(name) = segment.strip_prefix(':') {
            format!("path segment `{segment}` uses the old capture syntax: write `{{{name}}}`")
        } else if segment.starts_with('*') {
            format!("path segment `{segment}` uses the old capture syntax: write `{{{segment}}}`")
        } else if segment.matches('{').count() != segment.matches('}').count() {
            format!("path segment `{segment}` has unbalanced braces")
        } else if segment.contains('{')
            && !(segment.starts_with('{')
                && segment.ends_with('}')
                && segment.matches('{').count() == 1)
        {
            format!(
                "path segment `{segment}` mixes text and a parameter: a parameter is a whole segment"
            )
        } else {
            continue;
        };
        return Err(syn::Error::new(path.span(), problem));
    }
    Ok(())
}

/// The template a route answers, relative to the API prefix and version:
/// the controller `prefix` joined with `path`, or `path` alone when it is
/// absolute (`!/health`).
pub(crate) fn route_template(prefix: &str, path: &str) -> String {
    if path.starts_with('!') {
        return path.to_owned();
    }
    let absolute = prefix.starts_with('!');
    let mut out = String::from(if absolute { "!" } else { "" });
    for segment in [prefix.trim_start_matches('!'), path]
        .iter()
        .flat_map(|part| part.split('/'))
        .filter(|segment| !segment.is_empty())
    {
        out.push('/');
        out.push_str(segment);
    }
    if out.trim_start_matches('!').is_empty() {
        out.push('/');
    }
    out
}

/// The capture names in a template (`{id}` → `id`, `{*rest}` → `rest`).
pub(crate) fn params(template: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        names.push(rest[open + 1..open + close].trim_start_matches('*'));
        rest = &rest[open + close + 1..];
    }
    names
}

/// The template with capture names erased (`/deals/{id}` → `/deals/{}`,
/// `{*rest}` → `{*}`): two routes with the same method and shape collide.
pub(crate) fn shape(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        let wildcard = rest[open + 1..].starts_with('*');
        out.push_str(if wildcard { "{*}" } else { "{}" });
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_join_prefix_and_path() {
        assert_eq!(route_template("/deals", "/{id}"), "/deals/{id}");
        assert_eq!(route_template("/deals", "/"), "/deals");
        assert_eq!(route_template("", "/"), "/");
        assert_eq!(route_template("deals/", "notes"), "/deals/notes");
        assert_eq!(route_template("/deals", "!/health"), "!/health");
        assert_eq!(
            route_template("!/session", "/csrf-cookie"),
            "!/session/csrf-cookie"
        );
        assert_eq!(route_template("!/", "/"), "!/");
    }

    #[test]
    fn shapes_ignore_capture_names() {
        assert_eq!(shape("/deals/{id}"), shape("/deals/{deal}"));
        assert_eq!(shape("/teams/{team}/deals/{id}"), "/teams/{}/deals/{}");
        assert_eq!(shape("/files/{*path}"), "/files/{*}");
        assert_ne!(shape("/deals/{id}"), shape("/deals/{*id}"));
        assert_eq!(shape("/deals"), "/deals");
    }

    #[test]
    fn params_are_the_capture_names() {
        assert_eq!(params("/teams/{team}/deals/{id}"), ["team", "id"]);
        assert_eq!(params("/files/{*path}"), ["path"]);
        assert_eq!(params("/deals"), Vec::<&str>::new());
    }

    #[test]
    fn parameters_are_whole_segments() {
        let checked = |path: &str| {
            check(&LitStr::new(path, proc_macro2::Span::call_site())).map_err(|e| e.to_string())
        };
        assert!(checked("/deals/{id}/files/{*path}").is_ok());
        assert!(checked("!/health/{probe}").is_ok());
        for bad in ["/v{n}", "/{name}.json", "/{a}{b}", "/x{*rest}"] {
            let error = checked(bad).unwrap_err();
            assert!(
                error.contains("a parameter is a whole segment"),
                "{bad}: {error}"
            );
        }
        assert!(checked("/:id").unwrap_err().contains("old capture syntax"));
        assert!(checked("/{id").unwrap_err().contains("unbalanced"));
    }
}
