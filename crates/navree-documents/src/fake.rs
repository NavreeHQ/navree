//! [`DocumentsFake`]: documents for tests, rendered without an engine and
//! recorded.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_kernel::BoxFuture;
use serde_json::Value;

use crate::engine::{EngineKind, PdfEngine};
use crate::queue::QueuedDocument;
use crate::render::{Render, TemplateSource};
use crate::templates::{HTML_EXTENSION, Templates};
use crate::{Documents, DocumentsError, Pdf};

/// Documents for tests: every render succeeds at once, without the native
/// engine or Chrome, and is recorded.
///
/// The fake renders a plain PDF ([`Pdf::from_lines`]) whose first line is
/// the template's name (`inline` for inline markup) and whose next lines
/// are the data's top-level fields as `key: value`, sorted by key, so
/// `pdf.text()` shows what the template was given (Latin-1 only; other
/// characters print as `?`, so assert on [`rendered`](DocumentsFake::rendered)
/// for other scripts). Queued renders are recorded and not rendered.
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use navree_documents::DocumentsFake;
/// use serde_json::json;
///
/// let fake = DocumentsFake::new();
/// let documents = fake.documents(); // install with TestApp::builder().override_value(Arc::new(documents))
/// let pdf = documents
///     .pdf("certificates/classic", json!({ "name": "Ada", "score": 98 }))
///     .await
///     .unwrap();
/// assert_eq!(pdf.text().unwrap(), "certificates/classic\nname: Ada\nscore: 98");
/// fake.assert_rendered("certificates/classic");
/// fake.assert_rendered_with("certificates/classic", |render| render.data["name"] == "Ada");
/// # });
/// ```
#[derive(Clone, Debug, Default)]
pub struct DocumentsFake {
    state: Arc<FakeState>,
}

/// A render the fake queued.
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedRender {
    /// What was queued.
    pub render: Render,
    /// What [`Documents::queue`] returned.
    pub document: QueuedDocument,
}

#[derive(Debug, Default)]
pub(crate) struct FakeState {
    /// `None`: every name renders.
    templates: Option<Templates>,
    inner: Mutex<FakeInner>,
}

#[derive(Debug, Default)]
struct FakeInner {
    renders: Vec<Render>,
    queued: Vec<QueuedRender>,
    /// Template name → error message.
    failures: BTreeMap<String, String>,
}

impl FakeState {
    fn lock(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn accepts_any_template(&self) -> bool {
        self.templates.is_none()
    }

    pub(crate) fn record_queued(&self, render: &Render, document: &QueuedDocument) {
        self.lock().queued.push(QueuedRender {
            render: render.clone(),
            document: document.clone(),
        });
    }
}

impl DocumentsFake {
    /// A fake that renders any template name.
    pub fn new() -> Self {
        Self::default()
    }

    /// A fake that renders only the templates in `templates` (as
    /// `.html`); other names are [`DocumentsError::TemplateNotFound`], as
    /// with the real engines.
    pub fn with_templates(templates: Templates) -> Self {
        Self {
            state: Arc::new(FakeState {
                templates: Some(templates),
                inner: Mutex::default(),
            }),
        }
    }

    /// Renders of `template` fail with [`DocumentsError::Template`]
    /// carrying `message`, to test how your code handles a failed render.
    pub fn fail(self, template: &str, message: impl Into<String>) -> Self {
        self.state
            .lock()
            .failures
            .insert(template.to_owned(), message.into());
        self
    }

    /// The [`Documents`] backed by this fake. Install it with
    /// `TestApp::builder().override_value(Arc::new(fake.documents()))`;
    /// the fake keeps recording through every clone.
    pub fn documents(&self) -> Documents {
        let templates = self.state.templates.clone().unwrap_or_default();
        let engine = FakeEngine {
            kind: EngineKind::Native,
            state: self.state.clone(),
        };
        Documents::fake(templates, Arc::new(engine), self.state.clone())
    }

    /// The fake's engine on its own, as engine `kind` (for code that takes
    /// a [`PdfEngine`]). Its renders are recorded too.
    pub fn engine(&self, kind: EngineKind) -> FakeEngine {
        FakeEngine {
            kind,
            state: self.state.clone(),
        }
    }

    /// Every render, in order.
    pub fn rendered(&self) -> Vec<Render> {
        self.state.lock().renders.clone()
    }

    /// The names of every render, in order (`inline` for inline markup).
    pub fn rendered_names(&self) -> Vec<String> {
        self.state
            .lock()
            .renders
            .iter()
            .map(|render| render.name().to_owned())
            .collect()
    }

    /// Every queued render, in order.
    pub fn queued(&self) -> Vec<QueuedRender> {
        self.state.lock().queued.clone()
    }

    /// Panics unless `template` was rendered.
    #[track_caller]
    pub fn assert_rendered(&self, template: &str) {
        let names = self.rendered_names();
        assert!(
            names.iter().any(|name| same_template(name, template)),
            "the document `{template}` was not rendered; rendered: {names:?}"
        );
    }

    /// Panics unless `template` was rendered at least once with a render
    /// for which `check` is true (its data, locale, engine, title).
    #[track_caller]
    pub fn assert_rendered_with(&self, template: &str, check: impl Fn(&Render) -> bool) {
        let renders = self.rendered();
        let matching: Vec<&Render> = renders
            .iter()
            .filter(|render| same_template(render.name(), template))
            .collect();
        assert!(
            !matching.is_empty(),
            "the document `{template}` was not rendered; rendered: {:?}",
            self.rendered_names()
        );
        assert!(
            matching.iter().any(|render| check(render)),
            "the document `{template}` was rendered, but never as expected; data: {:?}",
            matching.iter().map(|r| &r.data).collect::<Vec<_>>()
        );
    }

    /// Panics if `template` was rendered.
    #[track_caller]
    pub fn assert_not_rendered(&self, template: &str) {
        let names = self.rendered_names();
        assert!(
            !names.iter().any(|name| same_template(name, template)),
            "the document `{template}` was rendered"
        );
    }

    /// Panics if anything was rendered.
    #[track_caller]
    pub fn assert_nothing_rendered(&self) {
        let names = self.rendered_names();
        assert!(names.is_empty(), "documents were rendered: {names:?}");
    }

    /// Panics unless `template` was queued.
    #[track_caller]
    pub fn assert_queued(&self, template: &str) {
        let queued: Vec<String> = self
            .queued()
            .iter()
            .map(|q| q.render.name().to_owned())
            .collect();
        assert!(
            queued.iter().any(|name| same_template(name, template)),
            "the document `{template}` was not queued; queued: {queued:?}"
        );
    }
}

/// Whether two template names are the same, with or without extension.
fn same_template(a: &str, b: &str) -> bool {
    let strip = |name: &str| -> String {
        let name = name.trim_start_matches('/');
        name.strip_suffix(&format!(".{HTML_EXTENSION}"))
            .unwrap_or(name)
            .to_owned()
    };
    strip(a) == strip(b)
}

/// The fake's [`PdfEngine`]: see [`DocumentsFake`].
#[derive(Clone, Debug)]
pub struct FakeEngine {
    kind: EngineKind,
    state: Arc<FakeState>,
}

impl FakeEngine {
    fn render_now(&self, render: &Render) -> Result<Pdf, DocumentsError> {
        self.state.lock().renders.push(render.clone());
        if let Some(error) = &render.data_error {
            return Err(DocumentsError::Data(error.clone()));
        }
        let name = render.name();
        if let TemplateSource::Template(template) = &render.source {
            self.check_exists(template)?;
        }
        let failure = self
            .state
            .lock()
            .failures
            .iter()
            .find(|(failing, _)| same_template(failing, name))
            .map(|(_, message)| message.clone());
        if let Some(message) = failure {
            return Err(DocumentsError::Template {
                file: name.to_owned(),
                line: None,
                column: None,
                message,
            });
        }
        let mut lines = vec![name.to_owned()];
        if let Value::Object(fields) = &render.data {
            let mut fields: Vec<(&String, &Value)> = fields.iter().collect();
            fields.sort_by(|a, b| a.0.cmp(b.0));
            for (key, value) in fields {
                let value = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                lines.push(format!("{key}: {value}"));
            }
        }
        let pdf = Pdf::from_lines(lines);
        match &render.title {
            Some(title) => pdf.with_metadata(title, None),
            None => Ok(pdf),
        }
    }

    /// With templates, `name` must exist as `name.html`.
    fn check_exists(&self, name: &str) -> Result<(), DocumentsError> {
        let Some(templates) = &self.state.templates else {
            return Ok(());
        };
        let tried: Vec<String> = Templates::template_path(name, HTML_EXTENSION)
            .into_iter()
            .collect();
        if tried.iter().any(|path| templates.exists(path)) {
            return Ok(());
        }
        Err(DocumentsError::TemplateNotFound {
            name: name.to_owned(),
            tried,
        })
    }
}

impl PdfEngine for FakeEngine {
    fn kind(&self) -> EngineKind {
        self.kind
    }

    fn render<'a>(&'a self, render: &'a Render) -> BoxFuture<'a, Result<Pdf, DocumentsError>> {
        Box::pin(async move { self.render_now(render) })
    }
}
