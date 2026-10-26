//! [`Field`]: what a resource shows and edits.

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use navree_validation::Messages;
use serde_json::Value;

use crate::Resource;
use crate::values::{self, is_blank};

/// A resource referred to by a relationship field or filter, resolved by
/// type when the panel boots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Related {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
}

impl Related {
    pub(crate) fn of<R: Resource>() -> Self {
        Self {
            type_id: TypeId::of::<R>(),
            type_name: std::any::type_name::<R>(),
        }
    }
}

/// A value computed from a model (`Field::computed`), kept without its
/// model type and called back with it.
#[derive(Clone)]
pub(crate) struct Computed {
    pub(crate) model: TypeId,
    pub(crate) model_name: &'static str,
    f: Arc<dyn Any + Send + Sync>,
}

/// The typed closure inside a [`Computed`].
type ComputeFn<M> = Arc<dyn Fn(&M) -> String + Send + Sync>;

impl Computed {
    fn new<M, F>(f: F) -> Self
    where
        M: 'static,
        F: Fn(&M) -> String + Send + Sync + 'static,
    {
        let f: ComputeFn<M> = Arc::new(f);
        Self {
            model: TypeId::of::<M>(),
            model_name: std::any::type_name::<M>(),
            f: Arc::new(f),
        }
    }

    /// The value for `model`; `None` when the closure takes another model.
    pub(crate) fn call<M: 'static>(&self, model: &M) -> Option<String> {
        self.f.downcast_ref::<ComputeFn<M>>().map(|f| f(model))
    }
}

/// Renders a validation message: (field, rule, params) to text.
type MessageFn<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> String;

/// The kinds of field.
#[derive(Clone)]
pub(crate) enum FieldKind {
    Id,
    Text,
    Textarea,
    Markdown,
    Number,
    Money { currency_column: String },
    Boolean,
    Select,
    Date,
    DateTime,
    Email,
    Url,
    File { disk: String },
    BelongsTo(Related),
    HasMany(Related),
    Computed(Computed),
    View { template: String },
}

impl FieldKind {
    /// The kind's name in the metadata API and templates.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            FieldKind::Id => "id",
            FieldKind::Text => "text",
            FieldKind::Textarea => "textarea",
            FieldKind::Markdown => "markdown",
            FieldKind::Number => "number",
            FieldKind::Money { .. } => "money",
            FieldKind::Boolean => "boolean",
            FieldKind::Select => "select",
            FieldKind::Date => "date",
            FieldKind::DateTime => "datetime",
            FieldKind::Email => "email",
            FieldKind::Url => "url",
            FieldKind::File { .. } => "file",
            FieldKind::BelongsTo(_) => "belongs_to",
            FieldKind::HasMany(_) => "has_many",
            FieldKind::Computed(_) => "computed",
            FieldKind::View { .. } => "view",
        }
    }
}

/// The largest upload a file field accepts unless
/// [`max_size`](Field::max_size) says otherwise (10 MiB).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// One field of a resource: a column of its entity (or, for `computed`,
/// `has_many` and `view`, something derived from the record), how it is
/// shown and how it is edited.
///
/// ```ignore
/// Field::text("title").required().max_length(200).sortable()
/// Field::money("amount_cents", "currency").label("Amount").min(0)
/// Field::belongs_to::<CompanyResource>("company_id").label("Company")
/// ```
#[derive(Clone)]
pub struct Field {
    pub(crate) name: String,
    pub(crate) kind: FieldKind,
    pub(crate) label: Option<String>,
    pub(crate) help: Option<String>,
    pub(crate) sortable: bool,
    pub(crate) readonly: bool,
    pub(crate) on_index: bool,
    pub(crate) on_detail: bool,
    pub(crate) on_forms: bool,
    pub(crate) required: bool,
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
    pub(crate) min_length: Option<usize>,
    pub(crate) max_length: Option<usize>,
    pub(crate) pattern: Option<String>,
    pub(crate) default: Option<Value>,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) badge: bool,
    pub(crate) accept: Vec<String>,
    pub(crate) max_size: u64,
}

impl Field {
    fn new(name: impl Into<String>, kind: FieldKind) -> Self {
        let (on_index, on_detail, on_forms, readonly) = match kind {
            FieldKind::Id => (true, true, false, true),
            FieldKind::HasMany(_) => (false, true, false, true),
            FieldKind::Computed(_) | FieldKind::View { .. } => (true, true, false, true),
            _ => (true, true, true, false),
        };
        Self {
            name: name.into(),
            kind,
            label: None,
            help: None,
            sortable: false,
            readonly,
            on_index,
            on_detail,
            on_forms,
            required: false,
            min: None,
            max: None,
            min_length: None,
            max_length: None,
            pattern: None,
            default: None,
            options: Vec::new(),
            badge: false,
            accept: Vec::new(),
            max_size: DEFAULT_MAX_FILE_BYTES,
        }
    }

    /// The primary key (never editable).
    pub fn id() -> Self {
        Self::new("", FieldKind::Id)
    }

    /// A text column: a text input.
    pub fn text(column: &str) -> Self {
        Self::new(column, FieldKind::Text)
    }

    /// Long text: its first line on the list, all of it on the detail page;
    /// a text area in forms.
    pub fn textarea(column: &str) -> Self {
        Self::new(column, FieldKind::Textarea)
    }

    /// Markdown, rendered without raw HTML; a text area with a preview in
    /// forms.
    pub fn markdown(column: &str) -> Self {
        Self::new(column, FieldKind::Markdown)
    }

    /// A number column: a number input.
    pub fn number(column: &str) -> Self {
        Self::new(column, FieldKind::Number)
    }

    /// An amount in minor units (`amount_cents`) in the currency held by
    /// `currency_column` (`USD` when the record has none): shown as
    /// `$12,500.00`, entered as a decimal and stored in minor units.
    pub fn money(column: &str, currency_column: &str) -> Self {
        Self::new(
            column,
            FieldKind::Money {
                currency_column: currency_column.to_owned(),
            },
        )
    }

    /// A boolean column: a check mark; a switch in forms.
    pub fn boolean(column: &str) -> Self {
        Self::new(column, FieldKind::Boolean)
    }

    /// One of [`options`](Field::options): the option's label (or a badge
    /// with [`badge`](Field::badge)); a select in forms.
    pub fn select(column: &str) -> Self {
        Self::new(column, FieldKind::Select)
    }

    /// A date column (`YYYY-MM-DD`): a date picker.
    pub fn date(column: &str) -> Self {
        Self::new(column, FieldKind::Date)
    }

    /// A date-time column, shown in the viewer's time zone: a date-time
    /// picker.
    pub fn datetime(column: &str) -> Self {
        Self::new(column, FieldKind::DateTime)
    }

    /// An email address: a `mailto:` link; validated in forms.
    pub fn email(column: &str) -> Self {
        Self::new(column, FieldKind::Email)
    }

    /// An `http(s)` URL: a link; validated in forms.
    pub fn url(column: &str) -> Self {
        Self::new(column, FieldKind::Url)
    }

    /// A file stored on the storage disk `disk`; the column holds its path.
    /// A download link (a preview for images); an upload in forms.
    pub fn file(column: &str, disk: &str) -> Self {
        Self::new(
            column,
            FieldKind::File {
                disk: disk.to_owned(),
            },
        )
    }

    /// The record of resource `R` whose key is in `column`: its title,
    /// linked to its page; a searchable picker of the records of `R` the
    /// user may read in forms.
    pub fn belongs_to<R: Resource>(column: &str) -> Self {
        Self::new(column, FieldKind::BelongsTo(Related::of::<R>()))
    }

    /// The records of resource `R` whose `foreign_key` column holds this
    /// record's key, in a panel on the detail page. Not in forms.
    pub fn has_many<R: Resource>(foreign_key: &str) -> Self {
        Self::new(foreign_key, FieldKind::HasMany(Related::of::<R>()))
    }

    /// The value `f` computes from the record. Not in forms.
    pub fn computed<M, F>(name: &str, f: F) -> Self
    where
        M: 'static,
        F: Fn(&M) -> String + Send + Sync + 'static,
    {
        Self::new(name, FieldKind::Computed(Computed::new(f)))
    }

    /// The app's template `template` (navree-view), rendered with
    /// `{record, field}`. Not in forms. Needs `ViewModule` in the app.
    pub fn view(name: &str, template: &str) -> Self {
        Self::new(
            name,
            FieldKind::View {
                template: template.to_owned(),
            },
        )
    }

    /// The label (default: the column name in sentence case).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Help text under the input in forms.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Lets the list sort by this column.
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    /// Shown, never editable.
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// Not on the list.
    pub fn hide_on_index(mut self) -> Self {
        self.on_index = false;
        self
    }

    /// Not on the detail page.
    pub fn hide_on_detail(mut self) -> Self {
        self.on_detail = false;
        self
    }

    /// Not in forms (and so never edited).
    pub fn hide_on_forms(mut self) -> Self {
        self.on_forms = false;
        self
    }

    /// Only in forms.
    pub fn only_on_forms(mut self) -> Self {
        self.on_index = false;
        self.on_detail = false;
        self.on_forms = true;
        self
    }

    /// Must not be empty (`title should not be empty`).
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// The smallest number allowed (for money, in minor units).
    pub fn min(mut self, n: impl Into<f64>) -> Self {
        self.min = Some(n.into());
        self
    }

    /// The largest number allowed (for money, in minor units).
    pub fn max(mut self, n: impl Into<f64>) -> Self {
        self.max = Some(n.into());
        self
    }

    /// The fewest characters allowed.
    pub fn min_length(mut self, n: usize) -> Self {
        self.min_length = Some(n);
        self
    }

    /// The most characters allowed.
    pub fn max_length(mut self, n: usize) -> Self {
        self.max_length = Some(n);
        self
    }

    /// A regular expression the text must match (checked when the panel
    /// boots).
    pub fn pattern(mut self, regex: &str) -> Self {
        self.pattern = Some(regex.to_owned());
        self
    }

    /// The value a create form starts with.
    pub fn default(mut self, value: impl Into<Value>) -> Self {
        self.default = Some(value.into());
        self
    }

    /// The options of a select field: `(value, label)` pairs.
    pub fn options<I, K, V>(mut self, options: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.options = options
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        self
    }

    /// Shows a select field's label as a badge.
    pub fn badge(mut self) -> Self {
        self.badge = true;
        self
    }

    /// The MIME types a file field accepts (`image/*`, `application/pdf`);
    /// any type by default.
    pub fn accept<I, S>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.accept = types.into_iter().map(Into::into).collect();
        self
    }

    /// The largest file a file field accepts, in bytes (10 MiB by default).
    pub fn max_size(mut self, bytes: u64) -> Self {
        self.max_size = bytes;
        self
    }

    /// The field's name: its column, or the name given to `computed` and
    /// `view` fields (the foreign key of a `has_many` field).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The label on screen.
    pub fn label_text(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| sentence_case(&self.name))
    }

    /// Whether the field is a column of the entity (not `computed`,
    /// `has_many` or `view`).
    pub(crate) fn is_column(&self) -> bool {
        !matches!(
            self.kind,
            FieldKind::HasMany(_) | FieldKind::Computed(_) | FieldKind::View { .. }
        )
    }

    /// Whether forms show the field.
    pub(crate) fn in_forms(&self) -> bool {
        self.on_forms && self.is_column() && !matches!(self.kind, FieldKind::Id)
    }

    /// Whether forms let the user change the field (before access rules).
    pub(crate) fn editable(&self) -> bool {
        self.in_forms() && !self.readonly
    }

    /// Checks a submitted value (already in its JSON form: money in minor
    /// units) against the field's rules, adding messages to `errors`.
    pub(crate) fn validate(
        &self,
        value: &Value,
        messages: &Messages,
        pattern: Option<&regex::Regex>,
        errors: &mut Vec<String>,
    ) {
        let field = self.name.as_str();
        let message = |rule: &str, params: &[(&str, String)]| messages.render(field, rule, params);
        if is_blank(value) {
            if self.required {
                errors.push(message("required", &[]));
            }
            return;
        }
        match &self.kind {
            FieldKind::Number | FieldKind::Money { .. } => {
                let Ok(n) = values::number(value) else {
                    errors.push(message("numeric", &[]));
                    return;
                };
                if let Some(min) = self.min
                    && n < min
                {
                    errors.push(message("min.numeric", &[("min", number_text(min))]));
                }
                if let Some(max) = self.max
                    && n > max
                {
                    errors.push(message("max.numeric", &[("max", number_text(max))]));
                }
            }
            FieldKind::Boolean => {
                if values::boolean(value).is_none() {
                    errors.push(message("boolean", &[]));
                }
            }
            FieldKind::Date => {
                if values::date(value).is_none() {
                    errors.push(message("date", &[]));
                }
            }
            FieldKind::DateTime => {
                if values::datetime(value, None).is_none() {
                    errors.push(message("date", &[]));
                }
            }
            FieldKind::Select => {
                let text = values::key_text(value).unwrap_or_default();
                if !self.options.is_empty() && !self.options.iter().any(|(v, _)| *v == text) {
                    let values: Vec<&str> = self.options.iter().map(|(v, _)| v.as_str()).collect();
                    errors.push(message("in", &[("values", values.join(", "))]));
                }
            }
            _ => {}
        }
        if let Some(text) = value.as_str() {
            self.validate_text(text, &message, pattern, errors);
        }
    }

    fn validate_text(
        &self,
        text: &str,
        message: MessageFn<'_>,
        pattern: Option<&regex::Regex>,
        errors: &mut Vec<String>,
    ) {
        let length = text.chars().count();
        if let Some(min) = self.min_length
            && length < min
        {
            errors.push(message("min.string", &[("min", min.to_string())]));
        }
        if let Some(max) = self.max_length
            && length > max
        {
            errors.push(message("max.string", &[("max", max.to_string())]));
        }
        if let (Some(regex), Some(source)) = (pattern, &self.pattern)
            && !regex.is_match(text)
        {
            errors.push(message("regex", &[("pattern", format!("/{source}/"))]));
        }
        match self.kind {
            FieldKind::Email if !is_email(text) => errors.push(message("email", &[])),
            FieldKind::Url if !is_http_url(text) => errors.push(message("url", &[])),
            _ => {}
        }
    }
}

impl fmt::Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Field")
            .field("name", &self.name)
            .field("kind", &self.kind.name())
            .finish_non_exhaustive()
    }
}

/// `amount_cents` as `Amount cents`.
pub(crate) fn sentence_case(name: &str) -> String {
    let words = name.replace(['_', '-'], " ");
    let words = words.trim();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `0`, `1.5`: a rule bound as the message shows it.
fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// A plausible email address: one `@`, something before it, a dot in the
/// domain, no spaces.
pub(crate) fn is_email(text: &str) -> bool {
    let text = text.trim();
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text.chars().any(char::is_whitespace)
}

/// An absolute `http://` or `https://` URL with a host.
pub(crate) fn is_http_url(text: &str) -> bool {
    let text = text.trim();
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            text.get(..scheme.len())
                .filter(|start| start.eq_ignore_ascii_case(scheme))
                .map(|_| &text[scheme.len()..])
        })
        .is_some_and(|rest| {
            !rest.is_empty()
                && !rest.starts_with('/')
                && !rest
                    .chars()
                    .any(|c| c.is_whitespace() || c == '<' || c == '>' || c == '"')
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn errors(field: &Field, value: Value) -> Vec<String> {
        let pattern = field
            .pattern
            .as_deref()
            .map(|p| regex::Regex::new(p).unwrap());
        let mut errors = Vec::new();
        field.validate(&value, &Messages::default(), pattern.as_ref(), &mut errors);
        errors
    }

    #[test]
    fn labels_default_to_the_column_in_sentence_case() {
        assert_eq!(
            Field::text("expected_close_date").label_text(),
            "Expected close date"
        );
        assert_eq!(
            Field::money("amount_cents", "currency")
                .label("Amount")
                .label_text(),
            "Amount"
        );
    }

    #[test]
    fn rules_use_the_validation_messages() {
        let title = Field::text("title").required().max_length(5);
        assert_eq!(errors(&title, json!("")), ["title should not be empty"]);
        assert_eq!(
            errors(&title, json!("Globex renewal")),
            ["title must be shorter than or equal to 5 characters"]
        );
        assert!(errors(&title, json!("Deal")).is_empty());

        let amount = Field::money("amount_cents", "currency").min(0).max(100);
        assert_eq!(
            errors(&amount, json!(-1)),
            ["amount_cents must not be less than 0"]
        );
        assert_eq!(
            errors(&amount, json!(101)),
            ["amount_cents must not be greater than 100"]
        );
        assert_eq!(
            errors(&amount, json!("x")),
            ["amount_cents must be a number"]
        );

        let code = Field::text("code").min_length(2).pattern("^[A-Z]+$");
        assert_eq!(
            errors(&code, json!("a")),
            [
                "code must be longer than or equal to 2 characters",
                "code must match /^[A-Z]+$/ regular expression"
            ]
        );

        let stage = Field::select("stage").options([("lead", "Lead"), ("won", "Won")]);
        assert_eq!(
            errors(&stage, json!("lost")),
            ["stage must be one of the following values: lead, won"]
        );
        assert_eq!(
            errors(&Field::email("email"), json!("nope")),
            ["email must be a valid email address"]
        );
        assert_eq!(
            errors(&Field::url("website"), json!("javascript:alert(1)")),
            ["website must be a URL address"]
        );
        assert_eq!(
            errors(&Field::date("due"), json!("26/10/2026")),
            ["due must be a valid ISO 8601 date string"]
        );
        assert!(errors(&Field::text("note"), json!("")).is_empty());
    }

    #[test]
    fn computed_fields_call_back_with_their_model() {
        struct Deal {
            stage: &'static str,
        }
        let Field {
            kind: FieldKind::Computed(computed),
            ..
        } = Field::computed("probability", |deal: &Deal| format!("{}!", deal.stage))
        else {
            panic!("not computed");
        };
        assert_eq!(
            computed.call(&Deal { stage: "won" }).as_deref(),
            Some("won!")
        );
        assert_eq!(computed.call(&"another model"), None);
    }

    #[test]
    fn visibility_modifiers() {
        let field = Field::text("notes").hide_on_index();
        assert!(!field.on_index && field.on_detail && field.in_forms());
        let field = Field::text("secret").only_on_forms();
        assert!(!field.on_index && !field.on_detail && field.in_forms());
        assert!(!Field::id().in_forms());
        assert!(!Field::datetime("created_at").readonly().editable());
    }
}
