//! HTML templates for both engines: rendered with navree-view, with the
//! render's data, `locale` and `dir` as variables.

use std::sync::{Arc, OnceLock};

use navree_view::{ViewError, ViewOptions, Views};
use serde_json::{Value, json};

use crate::DocumentsError;
use crate::render::{Render, TemplateSource};
use crate::templates::{HTML_EXTENSION, Templates};

/// An engine's HTML templates, loaded on first use and re-read when files
/// change.
#[derive(Default)]
pub(crate) struct HtmlTemplates {
    views: OnceLock<Arc<Views>>,
}

impl HtmlTemplates {
    /// The document `render` prints: its path in the template tree
    /// (`invoices/show.html`, or `inline.html`) and its HTML. Blocking:
    /// templates are read from disk.
    pub(crate) fn document(
        &self,
        templates: &Templates,
        render: &Render,
    ) -> Result<(String, String), DocumentsError> {
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        match &render.source {
            TemplateSource::Html(html) => Ok(("inline.html".to_owned(), html.clone())),
            TemplateSource::Template(name) => {
                let path = Templates::template_path(name, HTML_EXTENSION).ok_or_else(|| {
                    DocumentsError::TemplateNotFound {
                        name: name.clone(),
                        tried: Vec::new(),
                    }
                })?;
                let views = self.views(templates)?;
                let html = views
                    .render(&path, template_context(render))
                    .map_err(|error| view_error(name, error))?;
                Ok((path, html))
            }
        }
    }

    fn views(&self, templates: &Templates) -> Result<Arc<Views>, DocumentsError> {
        if let Some(views) = self.views.get() {
            return Ok(views.clone());
        }
        let options = ViewOptions {
            dirs: templates.view_dirs(),
            reload: true,
        };
        let views = Views::with_templates(&options, templates.embedded_html())
            .map_err(|error| view_error("", error))?;
        Ok(self.views.get_or_init(|| Arc::new(views)).clone())
    }
}

/// The variables of an HTML template: the data's fields (or `data` for
/// data that is not an object), plus `locale` and `dir`.
pub(crate) fn template_context(render: &Render) -> Value {
    let locale = Value::String(render.locale.clone().unwrap_or_else(|| "en".into()));
    let dir = Value::String(render.direction().into());
    match &render.data {
        Value::Object(map) => {
            let mut map = map.clone();
            map.entry("locale").or_insert(locale);
            map.entry("dir").or_insert(dir);
            Value::Object(map)
        }
        Value::Null => json!({ "locale": locale, "dir": dir }),
        other => json!({ "data": other, "locale": locale, "dir": dir }),
    }
}

/// A navree-view error as a [`DocumentsError`] naming the file.
pub(crate) fn view_error(name: &str, error: ViewError) -> DocumentsError {
    match error {
        ViewError::NotFound { tried, .. } => DocumentsError::TemplateNotFound {
            name: name.to_owned(),
            tried,
        },
        ViewError::Template { name, source } => {
            let file = source.name().map(str::to_owned).unwrap_or(name);
            let message = match source.detail() {
                Some(detail) => format!("{}: {detail}", source.kind()),
                None => source.kind().to_string(),
            };
            DocumentsError::Template {
                file,
                line: source.line(),
                column: None,
                message,
            }
        }
        ViewError::Io { path, source } => DocumentsError::Template {
            file: path.display().to_string(),
            line: None,
            column: None,
            message: source.to_string(),
        },
        other => DocumentsError::Template {
            file: name.to_owned(),
            line: None,
            column: None,
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_variables_add_locale_and_direction() {
        let render = Render::template("x")
            .data(json!({"name": "Anu"}))
            .locale("ar");
        assert_eq!(
            template_context(&render),
            json!({"name": "Anu", "locale": "ar", "dir": "rtl"})
        );
        let render = Render::template("x").data(json!({"locale": "custom"}));
        assert_eq!(
            template_context(&render),
            json!({"locale": "custom", "dir": "ltr"})
        );
        assert_eq!(
            template_context(&Render::template("x")),
            json!({"locale": "en", "dir": "ltr"})
        );
        assert_eq!(
            template_context(&Render::template("x").data([1, 2])),
            json!({"data": [1, 2], "locale": "en", "dir": "ltr"})
        );
    }

    #[test]
    fn templates_render_with_their_data_and_report_errors() {
        let broken = Templates::from_files([("broken.html", "<p>{{ name </p>")]);
        match HtmlTemplates::default().document(&broken, &Render::template("broken")) {
            Err(DocumentsError::Template { file, .. }) => assert_eq!(file, "broken.html"),
            other => panic!("{other:?}"),
        }
        let templates = Templates::from_files([("greeting.html", "<p>Hello {{ name }}</p>")]);
        let views = HtmlTemplates::default();
        let (path, html) = views
            .document(
                &templates,
                &Render::template("greeting").data(json!({"name": "Anu"})),
            )
            .unwrap();
        assert_eq!(path, "greeting.html");
        assert_eq!(html, "<p>Hello Anu</p>");
        let inline = views
            .document(&templates, &Render::html("<b>x</b>"))
            .unwrap();
        assert_eq!(inline, ("inline.html".to_owned(), "<b>x</b>".to_owned()));
        assert!(matches!(
            views.document(&templates, &Render::template("missing")),
            Err(DocumentsError::TemplateNotFound { .. })
        ));
    }
}
