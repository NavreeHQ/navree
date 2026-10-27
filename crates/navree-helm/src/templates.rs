//! The panel's own templates (navree-view, HTML-escaped), embedded in the
//! crate.

use navree_view::{ViewError, Views};

/// Every template, by name.
const TEMPLATES: [(&str, &str); 10] = [
    ("helm/layout.html", include_str!("templates/layout.html")),
    ("helm/macros.html", include_str!("templates/macros.html")),
    ("helm/login.html", include_str!("templates/login.html")),
    ("helm/error.html", include_str!("templates/error.html")),
    (
        "helm/dashboard.html",
        include_str!("templates/dashboard.html"),
    ),
    ("helm/list.html", include_str!("templates/list.html")),
    ("helm/detail.html", include_str!("templates/detail.html")),
    ("helm/form.html", include_str!("templates/form.html")),
    ("helm/action.html", include_str!("templates/action.html")),
    ("helm/page.html", include_str!("templates/page.html")),
];

/// The message page, separate so the array above stays a fixed size.
const MESSAGE: (&str, &str) = ("helm/message.html", include_str!("templates/message.html"));

/// The Settings screens (feature `settings`).
#[cfg(feature = "settings")]
const SETTINGS: [(&str, &str); 2] = [
    (
        "helm/settings.html",
        include_str!("templates/settings.html"),
    ),
    (
        "helm/settings_form.html",
        include_str!("templates/settings_form.html"),
    ),
];
#[cfg(not(feature = "settings"))]
const SETTINGS: [(&str, &str); 0] = [];

/// The panel's templates.
pub(crate) fn views() -> Result<Views, ViewError> {
    Views::from_templates(TEMPLATES.into_iter().chain([MESSAGE]).chain(SETTINGS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_compiles() {
        let views = views().unwrap();
        for (name, _) in TEMPLATES.iter().chain([&MESSAGE]).chain(SETTINGS.iter()) {
            assert!(views.exists(name), "{name}");
        }
    }
}
