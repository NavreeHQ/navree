//! The upload component: a drop area around a native file input, the list of files, and the
//! row the browser module copies for each file it sends.

use super::labels::UploadLabels;
use super::limits::UploadLimits;
use crate::components::{Attachment, Button, icon};
use crate::form::csrf_field;
use crate::html::{Href, LocalHref};
use crate::icons;
use crate::node::{Element, Node, Uses, el, into_node};
use crate::render::POST_FORM;

/// How large a file must be before it goes in chunks, if nothing says otherwise: 8 MB.
const CHUNK_FROM: u64 = 8 * 1024 * 1024;

/// A number as an attribute value.
fn number(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// A file the server already has, as a row of the list: its name, its size, a link to it
/// and, when you give an address for it, a button that removes it.
///
/// ```
/// use navree_ui::{UploadedFile, render};
///
/// let row = UploadedFile::new("Aadhaar-Gurpreet-Singh.pdf").size(240 * 1024).href("/files/17");
/// let html = render(&row.into());
/// assert!(html.contains("240 KB"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct UploadedFile {
    name: String,
    size: Option<u64>,
    href: Option<Href>,
    remove: Option<Href>,
    thumbnail: Option<String>,
}

impl UploadedFile {
    /// A file called `name`. The name is text to show and nothing else.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: None,
            href: None,
            remove: None,
            thumbnail: None,
        }
    }

    /// Shows its size.
    pub fn size(mut self, bytes: u64) -> Self {
        self.size = Some(bytes);
        self
    }

    /// Links the name to the file.
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Shows a small picture of the file before its name: an address on this site that
    /// serves a picture (the page's content security policy allows no other). Anything that
    /// is not an address on this site is ignored. The picture has no description: the name
    /// beside it says what the file is.
    pub fn thumbnail(mut self, src: impl AsRef<str>) -> Self {
        self.thumbnail = LocalHref::parse(src.as_ref())
            .ok()
            .map(|href| href.as_str().to_owned());
        self
    }

    /// Adds a Remove button that posts to `href`, through the page's one post form. It
    /// works with no script, and the page then loads the core runtime for the post.
    pub fn remove(mut self, href: impl Into<Href>) -> Self {
        self.remove = Some(href.into());
        self
    }

    /// The row, in `labels`' words.
    pub fn row(&self, labels: &UploadLabels) -> Node {
        let mut attachment = Attachment::new(self.name.clone());
        if let Some(size) = self.size {
            attachment = attachment.meta(UploadLimits::size_text(size));
        }
        if let Some(href) = &self.href {
            attachment = attachment.href(href.clone());
        }
        if let Some(remove) = &self.remove {
            let button = row_button(&labels.remove, &self.name, "submit")
                .attr("form", POST_FORM)
                .attr("formaction", remove.clone())
                .attr("formmethod", "post")
                .uses(Uses::CORE);
            attachment = attachment.action(button);
        }
        let picture = self.thumbnail.as_ref().map(|src| {
            el("img")
                .slot("upload-preview")
                .attr("src", Href::from(src.as_str()))
                .attr("alt", "")
                .attr("width", 40_i64)
                .attr("height", 40_i64)
                .attr("loading", "lazy")
        });
        el("li")
            .slot("upload-item")
            .attr("data-state", "done")
            .children((picture, attachment))
            .into()
    }

    fn into_node(self) -> Node {
        self.row(&UploadLabels::default())
    }
}

into_node!(UploadedFile);

/// A small ghost button of `kind` (`button` or `submit`) whose name ends with the file's,
/// written in a span only assistive technology reads.
fn row_button(label: &str, file: &str, kind: &'static str) -> Element {
    el("button")
        .slot("button")
        .attr("type", kind)
        .attr("data-variant", "ghost")
        .attr("data-size", "sm")
        .children((
            label.to_owned(),
            el("span")
                .flag("data-ui-sr", true)
                .child(format!(" {file}")),
        ))
}

/// A file field with a drop area, a list of the files and, in a form of its own, progress.
///
/// With an [`action`](Self::action), the upload is a form that posts its files as
/// `multipart/form-data`: it works with JavaScript off (choose, then press Send), and with
/// the `upload` module each file starts on its way as soon as it is chosen or dropped,
/// with a progress bar, a way to stop it and to try again, and large files in chunks.
/// Without an action it is a field for the form around it, which must be a multipart
/// form, and the module lists the files that were chosen.
///
/// The limits you give are hints for the browser. The server holds every file to them: see
/// `Uploads`, which receives the files.
///
/// ```
/// use navree_ui::{Upload, UploadLimits, render};
///
/// let upload = Upload::new("photo", "Profile photo")
///     .action("/profile/photo")
///     .limits(UploadLimits::new(2 * 1024 * 1024).accept(["image/png", "image/jpeg"]));
/// let html = render(&upload.into());
/// assert!(html.contains(r#"enctype="multipart/form-data""#));
/// assert!(html.contains("PNG or JPG, up to 2 MB"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Upload {
    name: String,
    label: Node,
    id: Option<String>,
    action: Option<Href>,
    csrf: Option<String>,
    limits: UploadLimits,
    labels: UploadLabels,
    manual: bool,
    chunks: Option<String>,
    chunk_from: u64,
    refresh: Option<String>,
    hint: Option<Node>,
    error: Option<String>,
    required: bool,
    disabled: bool,
    compact: bool,
    previews: bool,
    files: Vec<UploadedFile>,
    send: Option<String>,
    key: Option<String>,
}

impl Upload {
    /// A field posting its files as `name`, called `label`.
    pub fn new(name: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            id: None,
            action: None,
            csrf: None,
            limits: UploadLimits::default(),
            labels: UploadLabels::default(),
            manual: false,
            chunks: None,
            chunk_from: CHUNK_FROM,
            refresh: None,
            hint: None,
            error: None,
            required: false,
            disabled: false,
            compact: false,
            previews: false,
            files: Vec::new(),
            send: None,
            key: None,
        }
    }

    /// The same upload posting as `name`: a form's file field takes the field's name.
    pub(crate) fn named(mut self, name: &str) -> Self {
        name.clone_into(&mut self.name);
        self
    }

    /// Sets the id (`upload-{name}` otherwise). The file input's is `{id}-input`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Makes the upload a form of its own that posts to `href`.
    pub fn action(mut self, href: impl Into<Href>) -> Self {
        self.action = Some(href.into());
        self
    }

    /// Puts the session's CSRF token in the form as the field `_csrf`, which a post with no
    /// script sends. The module sends the token it finds in the `XSRF-TOKEN` cookie. A field
    /// inside a form of yours leaves the token to that form.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// Sets what is accepted. The default is ten megabytes, one file, any kind.
    pub fn limits(mut self, limits: UploadLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: UploadLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Waits for the Send button instead of sending a file as soon as it is chosen.
    pub fn manual(mut self) -> Self {
        self.manual = true;
        self
    }

    /// Sends large files in chunks through `url`, an address on this site that is served by
    /// `Uploads::begin` and its companions. Anything that is not an address on this site is
    /// ignored.
    pub fn chunks(mut self, url: impl AsRef<str>) -> Self {
        self.chunks = LocalHref::parse(url.as_ref())
            .ok()
            .map(|href| href.as_str().to_owned());
        self
    }

    /// Inside a form of yours: the module sends each chosen file at once to `url`, an
    /// address on this site whose route stores it and answers with its key, made for the person
    /// sending ([`Received::into_keyed_response_for`](crate::Received::into_keyed_response_for)),
    /// and puts the key in a hidden field named like the upload, which the form posts in place
    /// of the file. With no script the file goes with the form, which must be a multipart form.
    /// Anything that is not an address on this site is ignored.
    pub fn send_to(mut self, url: impl AsRef<str>) -> Self {
        self.send = LocalHref::parse(url.as_ref())
            .ok()
            .map(|href| href.as_str().to_owned());
        self
    }

    /// The key of a file already sent ([`file_key_for`](crate::upload::file_key_for)), which the
    /// form posts again: for a form drawn again after it was refused, so the file is not
    /// chosen twice. The field is then not required in the browser.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into()).filter(|key| !key.is_empty());
        self
    }

    /// Sets how large a file must be to go in chunks (8 MB by default).
    pub fn chunk_from(mut self, bytes: u64) -> Self {
        self.chunk_from = bytes.max(1);
        self
    }

    /// Names a region (by its id) to fetch again when files have been sent, so the list it
    /// draws shows them.
    pub fn refresh(mut self, region: impl Into<String>) -> Self {
        self.refresh = Some(region.into());
        self
    }

    /// Adds a line after the limits.
    pub fn hint(mut self, hint: impl Into<Node>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Shows an error under the drop area and marks the field invalid.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Marks the field required.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Disables the field.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A smaller drop area, for a form with little room.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Shows a small picture of each chosen file that is a picture (PNG, JPEG, GIF, WebP or
    /// AVIF, up to 2 MB), before it is sent. The browser reads the file into a `data:` address,
    /// which the page's content security policy allows where it allows no `blob:`; a file
    /// that is larger, or another kind, is listed with its name only. A file the server
    /// already has gets its picture from [`UploadedFile::thumbnail`].
    pub fn previews(mut self) -> Self {
        self.previews = true;
        self
    }

    /// Lists a file the server already has.
    pub fn file(mut self, file: UploadedFile) -> Self {
        self.files.push(file);
        self
    }

    /// Lists the files the server already has.
    pub fn files(mut self, files: impl IntoIterator<Item = UploadedFile>) -> Self {
        self.files.extend(files);
        self
    }

    fn into_node(self) -> Node {
        let id = self
            .id
            .clone()
            .unwrap_or_else(|| format!("upload-{}", self.name));
        let input_id = format!("{id}-input");
        let label_id = format!("{id}-label");
        let hint_id = format!("{id}-hint");
        let status_id = format!("{id}-status");
        let error_id = format!("{id}-error");
        let standalone = self.action.is_some();
        let several = self.limits.files() > 1;
        let labels = &self.labels;

        let mut described = format!("{hint_id} {status_id}");
        if self.error.is_some() {
            described.push(' ');
            described.push_str(&error_id);
        }
        let accept = self.limits.types().join(",");
        let input = el("input")
            .slot("upload-input")
            .attr("id", input_id)
            .attr("type", "file")
            .attr("name", self.name.clone())
            .attr_opt("accept", (!accept.is_empty()).then_some(accept))
            .flag("multiple", several)
            .flag("required", self.required && self.key.is_none())
            .flag("disabled", self.disabled)
            .attr("aria-labelledby", label_id.clone())
            .attr("aria-describedby", described)
            .attr_opt("aria-invalid", self.error.is_some().then_some("true"));

        let (choose, drop) = if several {
            (&labels.choose, &labels.drop)
        } else {
            (&labels.choose_one, &labels.drop_one)
        };
        let prompt = el("span").slot("upload-prompt").children((
            icon(icons::UPLOAD),
            el("span").slot("upload-title").child(choose.clone()),
            el("span").child(drop.clone()),
        ));
        let mut hint_nodes: Vec<Node> = vec![Node::from(self.limits.sentence(labels))];
        if let Some(extra) = self.hint.clone() {
            hint_nodes.push(Node::from(" "));
            hint_nodes.push(extra);
        }
        let hint = el("span")
            .slot("upload-hint")
            .attr("id", hint_id)
            .children(hint_nodes);
        let zone = el("div")
            .slot("upload-zone")
            .children((prompt, hint, input));

        let label = el("span")
            .slot("upload-label")
            .attr("id", label_id)
            .child(self.label.clone())
            .child(self.required.then(|| {
                el("span")
                    .slot("field-required")
                    .attr("aria-hidden", "true")
                    .child(" *")
            }));
        let error = self.error.clone().map(|error| {
            el("p")
                .slot("field-error")
                .attr("id", error_id)
                .child(error)
        });
        let list = el("ul")
            .slot("upload-list")
            .attr("aria-label", labels.files.clone())
            .children(
                self.files
                    .iter()
                    .map(|file| file.row(labels))
                    .collect::<Vec<Node>>(),
            );
        let status = el("div")
            .slot("upload-status")
            .attr("id", status_id)
            .attr("role", "status");
        let actions = standalone.then(|| {
            el("div").slot("upload-actions").child(
                Button::primary(labels.send.clone())
                    .submit()
                    .disabled(self.disabled),
            )
        });
        let csrf = if standalone {
            self.csrf.clone().map(csrf_field)
        } else {
            None
        };
        // In a form of yours, the key of the file the module sent goes in its place.
        let send = self.send.clone().filter(|_| !standalone);
        let key = (!standalone && (send.is_some() || self.key.is_some())).then(|| {
            el("input")
                .slot("upload-key")
                .attr("type", "hidden")
                .attr("name", self.name.clone())
                .attr("value", self.key.clone().unwrap_or_default())
        });

        let mut root = el(if standalone { "form" } else { "div" })
            .slot("upload")
            .attr("id", id);
        if let Some(action) = &self.action {
            root = root
                .attr("method", "post")
                .attr("action", action.clone())
                .attr("enctype", "multipart/form-data");
        }
        root.flag("data-ui-upload", true)
            .attr("data-max-bytes", number(self.limits.bytes()))
            .attr("data-max-files", number(self.limits.files() as u64))
            .attr_opt("data-chunks", self.chunks.clone())
            .attr_opt(
                "data-chunk-from",
                self.chunks.as_ref().map(|_| number(self.chunk_from)),
            )
            .attr_opt("data-refresh", self.refresh.clone())
            .attr_opt("data-send", send)
            .flag("data-required", self.required)
            .flag("data-manual", self.manual && standalone)
            .flag("data-compact", self.compact)
            .flag("data-previews", self.previews)
            .flag("data-disabled", self.disabled)
            .flag("data-invalid", self.error.is_some())
            .attr("data-ui-keep-attrs", "data-ready")
            .attr("data-l", labels.module_words())
            .uses(Uses::UPLOAD)
            .children((
                csrf,
                key,
                label,
                zone,
                error,
                list,
                status,
                actions,
                row_template(labels, self.previews),
            ))
            .into()
    }
}

into_node!(Upload);

/// The row the module copies for each file it sends, with the page's words and icon in it.
fn row_template(labels: &UploadLabels, previews: bool) -> Element {
    let picture = previews.then(|| {
        el("img")
            .slot("upload-preview")
            .attr("alt", "")
            .attr("width", 40_i64)
            .attr("height", 40_i64)
            .flag("hidden", true)
    });
    let row = el("li")
        .slot("upload-item")
        .attr("data-state", "queued")
        .children((
            picture,
            Attachment::new("").meta(""),
            el("progress")
                .slot("progress")
                .attr("max", 100_i64)
                .attr("value", 0_i64),
            el("div").slot("upload-row-actions").children((
                row_button(&labels.cancel, "", "button").flag("data-ui-cancel", true),
                row_button(&labels.retry, "", "button").flag("data-ui-retry", true),
                row_button(&labels.remove, "", "button").flag("data-ui-remove", true),
            )),
        ));
    el("template").slot("upload-row").child(row)
}
