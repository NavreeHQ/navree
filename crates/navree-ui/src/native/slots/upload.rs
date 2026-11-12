//! `Upload`: files a person sends, through the file field.
//!
//! An upload with scripts is a drop zone that sends files in chunks and draws
//! their progress. Under it the page keeps a plain `<input type="file">`, and
//! that is what is read: a client picks files from the device, its camera or
//! its files app, and sends them in the form like any other field. The zone's
//! prompt, the status the script writes and the row it copies for each file it
//! sends are the browser's.
//!
//! An upload that stands alone is a form of its own (a post that carries the
//! files, and the page's token); inside a form of yours it is one field of it.
//! The files the server already has are rows under the field, each with a way
//! to open it and, when the page allows, to remove it.

use super::super::lower::{Control, Cx, N, Parts, Read, attr, elements, fail, hidden_in, hooks, id, slot, text_of};
use crate::node::{Element, Node};
use navree_native::node as n;

/// The hooks the root of an upload carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-upload"];

/// `Upload`.
pub(crate) fn upload(cx: &mut Cx, element: &Element) -> Read {
    let standalone = element.tag() == "form";
    let mut parts = Parts::of(element);
    let label = parts.slot("upload-label");
    let zone = parts.slot("upload-zone");
    let error = parts.slot("field-error");
    let list = parts.slot("upload-list");
    // What the script writes as files are sent, and the row it copies for each.
    let _status = parts.slot("upload-status");
    let _template = parts.slot("upload-row");
    let actions = parts.slot("upload-actions");
    // The key of the file the script sent, kept in a form of yours.
    let _key = parts.slot("upload-key");
    // The token of a standalone upload is a hidden field with no slot.
    for node in parts.left() {
        match node {
            Node::Element(input)
                if input.tag() == "input" && attr(input, "type").as_deref() == Some("hidden") => {}
            Node::Text(text) if text.trim().is_empty() => {}
            _ => return fail("a part of an upload no reader here knows"),
        }
    }
    let (Some(label), Some(zone)) = (label, zone) else {
        return fail("an upload with no label or no zone");
    };
    let field = field_of(cx, label, zone, error)?;
    let mut out = vec![N::Field(field)];
    if let Some(list) = list {
        hooks(list, &[])?;
        let files = files_of(cx, list)?;
        if !files.is_empty() {
            out.push(N::List(n::List {
                id: id(list),
                children: files,
                ..n::List::default()
            }));
        }
    }
    if !standalone {
        return Ok(out);
    }
    if let Some(actions) = actions {
        hooks(actions, &[])?;
        out.extend(cx.as_form(|cx| cx.of(actions)));
    }
    let mut hidden = Vec::new();
    hidden_in(element.child_nodes(), &mut hidden);
    Ok(vec![N::Form(n::Form {
        id: id(element),
        method: n::Method::Post,
        action: attr(element, "action")
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_default(),
        hidden,
        children: out,
    })])
}

/// The field: the file input, named by the label, with the words under it.
fn field_of(
    cx: &mut Cx,
    label: &Element,
    zone: &Element,
    error: Option<&Element>,
) -> Read<n::Field> {
    hooks(zone, &[])?;
    let mut parts = Parts::of(zone);
    // The words that ask for a file, and the line that says what is taken.
    let _prompt = parts.slot("upload-prompt");
    let hint = parts.slot("upload-hint");
    let input = parts.slot("upload-input");
    parts.done()?;
    let Some(input) = input else {
        return fail("an upload with no file field");
    };
    let Control::Field(mut field) = cx.control(input)? else {
        return fail("an upload whose input is not a file field");
    };
    if !matches!(field.input, navree_native::input::Input::File(_)) {
        return fail("an upload whose input is not a file field");
    }
    field.label = Some(text_of(label).trim_end_matches('*').trim().to_owned());
    field.help = hint.map(text_of).filter(|words| !words.is_empty());
    field.error = error.map(text_of);
    Ok(*field)
}

/// The files the server has already: a row each.
fn files_of(cx: &mut Cx, list: &Element) -> Read<Vec<N>> {
    let mut rows = Vec::new();
    for item in elements(list)? {
        if slot(item).as_deref() != Some("upload-item") {
            return fail("something that is not a file in a list of files");
        }
        hooks(item, &[])?;
        let mut parts = Parts::of(item);
        let picture = parts.slot("upload-preview");
        let file = parts.slot("attachment");
        parts.done()?;
        let Some(file) = file else {
            return fail("a file with no row");
        };
        let mut read = Vec::new();
        cx.element(file, &mut read);
        let [N::Row(row)] = read.as_mut_slice() else {
            return fail("a file whose row is not one row");
        };
        if let Some(picture) = picture
            && let Some(src) = attr(picture, "src")
        {
            row.leading = Some(Box::new(N::Image(n::Image {
                src: src.into_owned(),
                alt: String::new(),
                width: Some(40),
                height: Some(40),
                ..n::Image::default()
            })));
        }
        rows.extend(read);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use navree_native::input as i;

    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::native::TestNative;
    use crate::{Text, Upload, UploadedFile, View};

    fn field_of(node: &N) -> &n::Field {
        let N::Field(field) = node else {
            panic!("not a field: {node:?}");
        };
        field
    }

    #[test]
    fn an_upload_is_a_file_field_with_its_label_hint_and_error() {
        let node = read(
            Upload::new("avatar", "Photo")
                .hint(Text::new("Square is best"))
                .error("That one is too big")
                .required(),
        );
        // A field, and nothing else when no file is listed.
        let N::Stack(stack) = &node else {
            let field = field_of(&node);
            assert_field(field);
            return;
        };
        assert_field(field_of(&stack.children[0]));
    }

    fn assert_field(field: &n::Field) {
        assert_eq!(field.name, "avatar");
        assert_eq!(field.label.as_deref(), Some("Photo"));
        assert!(field.required);
        assert_eq!(field.error.as_deref(), Some("That one is too big"));
        assert!(
            field
                .help
                .as_deref()
                .is_some_and(|help| help.contains("Square is best") && help.contains("10 MB")),
            "{:?}",
            field.help
        );
        assert!(matches!(&field.input, i::Input::File(file) if !file.multiple));
    }

    #[test]
    fn what_is_taken_and_how_many_come_from_the_limits() {
        let N::Field(field) = read(
            Upload::new("docs", "Documents").limits(
                crate::UploadLimits::new(10 << 20)
                    .max_files(5)
                    .accept(["image/png", "application/pdf"]),
            ),
        ) else {
            panic!("not a field");
        };
        let i::Input::File(file) = field.input else {
            panic!("not a file field");
        };
        assert!(file.multiple);
        assert_eq!(file.accept.as_deref(), Some("image/png,application/pdf"));
    }

    #[test]
    fn a_standalone_upload_is_a_form_that_posts_the_files_and_the_token() {
        let page = TestNative::of(
            &View::new("Documents").body(
                Upload::new("docs", "Documents")
                    .action("/docs")
                    .csrf("tok-1")
                    .file(
                        UploadedFile::new("invoice.pdf")
                            .size(1234)
                            .href("/files/1")
                            .remove("/files/1/remove"),
                    ),
            ),
        );
        assert!(page.is_native(), "{:?}", page.why());
        let Some(n::Node::Form(form)) = page.find("upload-docs") else {
            panic!("no form: {:?}", page.screen().body);
        };
        assert_eq!(form.method, n::Method::Post);
        assert_eq!(form.action, "/docs");
        assert_eq!(
            form.hidden,
            [n::Hidden {
                name: "_csrf".into(),
                value: "tok-1".into(),
                id: None
            }]
        );
        let [N::Field(field), N::List(files), N::Button(send)] = form.children.as_slice() else {
            panic!("not a field, files and a button: {:?}", form.children);
        };
        assert_eq!(field.name, "docs");
        assert_eq!(send.label, "Send");
        let [N::Row(row)] = files.children.as_slice() else {
            panic!("not one file");
        };
        assert_eq!(row.title, "invoice.pdf");
        assert_eq!(row.subtitle.as_deref(), Some("1.2 KB"));
        assert_eq!(row.action, Some(navree_native::Action::navigate("/files/1")));
        // Taking it away posts with the page's token.
        let Some(N::Button(remove)) = row.trailing.as_deref() else {
            panic!("no way to remove it");
        };
        assert!(matches!(
            &remove.action,
            Some(navree_native::Action::Request { href, .. }) if href == "/files/1/remove"
        ));
    }

    #[test]
    fn an_upload_in_a_form_of_yours_is_one_field_of_it() {
        let N::Form(form) = read(
            crate::el("form")
                .attr("method", "post")
                .attr("action", "/profile")
                .child(Upload::new("avatar", "Photo")),
        ) else {
            panic!("not a form");
        };
        assert!(matches!(form.children.as_slice(), [N::Field(field)] if field.name == "avatar"));
    }

    #[test]
    fn a_hook_no_reader_knows_inside_an_upload_is_still_a_miss() {
        for name in ["upload", "upload-list", "upload-zone"] {
            let node: Node = Upload::new("docs", "Documents")
                .file(UploadedFile::new("a.pdf"))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
