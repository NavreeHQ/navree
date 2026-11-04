//! The upload component: its markup, its words and limits, what a page loads for it, and
//! that hostile text stays text. The server side is in `upload_server.rs`.

use navree_ui::{
    Upload, UploadLabels, UploadLimits, UploadedFile, Uses, View, assets, render,
};

const MB: u64 = 1024 * 1024;

fn photo() -> Upload {
    Upload::new("photo", "Profile photo")
        .action("/profile/photo")
        .limits(UploadLimits::new(2 * MB).accept(["image/png", "image/jpeg"]))
}

fn html(upload: Upload) -> String {
    render(&upload.into())
}

// UI-GENUI-021: a standalone upload is a real multipart form, so it posts with JavaScript off.
#[test]
fn an_upload_with_an_action_is_a_multipart_form_around_a_native_file_input() {
    let out = html(photo());
    assert!(out.starts_with(r#"<form data-slot="upload" id="upload-photo" method="post" action="/profile/photo" enctype="multipart/form-data""#), "{out}");
    assert!(out.contains("data-ui-upload"), "{out}");
    assert!(out.contains(r#"<input data-slot="upload-input" id="upload-photo-input" type="file" name="photo""#), "{out}");
    assert!(out.contains(r#"accept="image/png,image/jpeg""#), "{out}");
    // JavaScript off: choose, then press Send.
    assert!(out.contains(r#"type="submit""#) && out.contains("Send"), "{out}");
    assert!(out.contains(r#"data-slot="upload-actions""#), "{out}");
}

// UI-GENUI-021: without an action the upload is a field inside the caller's form.
#[test]
fn an_upload_without_an_action_is_a_field_for_the_form_around_it() {
    let out = html(Upload::new("resume", "Résumé").limits(UploadLimits::new(5 * MB)));
    assert!(out.starts_with(r#"<div data-slot="upload""#), "{out}");
    assert!(!out.contains("<form"), "{out}");
    assert!(!out.contains("enctype"), "{out}");
    assert!(!out.contains(r#"type="submit""#), "{out}");
    assert!(out.contains(r#"type="file" name="resume""#), "{out}");
}

// UI-GENUI-021: the input is named by the label, described by the hint and the status.
#[test]
fn the_input_is_named_and_described() {
    let out = html(photo());
    assert!(out.contains(r#"<span data-slot="upload-label" id="upload-photo-label">Profile photo</span>"#), "{out}");
    assert!(out.contains(r#"aria-labelledby="upload-photo-label""#), "{out}");
    assert!(out.contains(r#"aria-describedby="upload-photo-hint upload-photo-status""#), "{out}");
    assert!(out.contains(r#"id="upload-photo-hint""#), "{out}");
    assert!(out.contains(r#"id="upload-photo-status" role="status""#), "{out}");
    let custom = html(Upload::new("a", "A").id("docs"));
    assert!(custom.contains(r#"id="docs""#) && custom.contains(r#"id="docs-input""#), "{custom}");
}

// UI-GENUI-021: the limits are said in words, and a single file does not say "one file".
#[test]
fn the_limits_are_written_as_a_sentence() {
    let out = html(photo());
    assert!(out.contains("PNG or JPG, up to 2 MB"), "{out}");
    assert!(!out.contains(", up to 1"), "a single file has no count: {out}");
    assert!(!out.contains(" multiple"), "{out}");
    let many = html(
        Upload::new("docs", "Documents")
            .limits(UploadLimits::new(10 * MB).max_files(5).accept(["application/pdf", "image/*", "text/csv"])),
    );
    assert!(many.contains("PDF, images or CSV, up to 10 MB, up to 5 files"), "{many}");
    assert!(many.contains(" multiple"), "{many}");
    assert!(many.contains(r#"accept="application/pdf,image/*,text/csv""#), "{many}");
    let any = html(Upload::new("x", "X").limits(UploadLimits::new(1536)));
    assert!(any.contains("up to 1.5 KB"), "{any}");
    assert!(!any.contains("accept="), "{any}");
}

// UI-GENUI-021: the limits object itself.
#[test]
fn limits_are_clamped_and_cleaned() {
    assert_eq!(UploadLimits::new(10).max_files(0).files(), 1);
    assert_eq!(UploadLimits::new(10).max_files(10_000).files(), 100);
    assert_eq!(UploadLimits::new(0).bytes(), 1);
    // Only `type/subtype` and `type/*` stay: nothing a hostile string could carry into an attribute.
    let limits = UploadLimits::new(10).accept([
        "image/png",
        "x\" onfocus=\"alert(1)",
        "text/*",
        "",
        "noslash",
        "IMAGE/WEBP",
        "a/b/c",
    ]);
    assert_eq!(limits.types().to_vec(), vec!["image/png", "text/*", "image/webp"]);
    assert!(limits.allows("image/png") && limits.allows("text/csv") && limits.allows("Image/WebP"));
    assert!(!limits.allows("application/pdf") && !limits.allows("text"));
    assert!(UploadLimits::new(10).allows("anything/at-all"));
}

// UI-GENUI-021: sizes read the same on the server and in the page.
#[test]
fn sizes_are_written_in_binary_units_without_a_trailing_zero() {
    for (bytes, text) in [
        (0, "0 B"),
        (1023, "1023 B"),
        (1024, "1 KB"),
        (1536, "1.5 KB"),
        (10 * 1024, "10 KB"),
        (300 * 1024 + 300, "300 KB"),
        (MB, "1 MB"),
        (MB * 5 / 2, "2.5 MB"),
        (1024 * MB, "1 GB"),
    ] {
        assert_eq!(UploadLimits::size_text(bytes), text, "{bytes}");
    }
}

// UI-GENUI-021: the page learns the limits as hints; the server decides.
#[test]
fn the_markup_carries_the_limits_as_hints_for_the_module() {
    let out = html(
        photo()
            .chunks("/uploads/chunks")
            .chunk_from(4 * MB)
            .refresh("photos")
            .manual(),
    );
    assert!(out.contains(r#"data-max-bytes="2097152""#), "{out}");
    assert!(out.contains(r#"data-max-files="1""#), "{out}");
    assert!(out.contains(r#"data-chunks="/uploads/chunks""#), "{out}");
    assert!(out.contains(r#"data-chunk-from="4194304""#), "{out}");
    assert!(out.contains(r#"data-refresh="photos""#), "{out}");
    assert!(out.contains("data-manual"), "{out}");
    let plain = html(photo());
    assert!(!plain.contains("data-chunks") && !plain.contains("data-manual") && !plain.contains("data-refresh"), "{plain}");
}

// UI-GENUI-021: a chunk address from outside the site goes nowhere.
#[test]
fn a_chunk_address_that_leaves_the_site_is_refused() {
    for bad in ["javascript:alert(1)", "//evil.example/c", "https://evil.example/c", "/\\evil.example"] {
        let out = html(photo().chunks(bad));
        assert!(!out.contains("evil.example") && !out.contains("javascript"), "{bad}: {out}");
        assert!(!out.contains("data-chunks"), "{bad}: {out}");
    }
    let out = html(photo().action("javascript:alert(1)"));
    assert!(!out.contains("javascript"), "{out}");
}

// UI-GENUI-021: the words reach the module as JSON, and every word is the app's to translate.
#[test]
fn the_words_for_the_module_are_json_and_translatable() {
    let labels = UploadLabels {
        sending: "{name} भेजी जा रही है".into(),
        ..UploadLabels::default()
    };
    let out = html(photo().labels(labels));
    let json = out
        .split(r#"data-l=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("data-l")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let words: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}: {json}"));
    for key in ["sending", "done", "failed", "offline", "too_big", "too_many", "empty", "removed"] {
        assert!(words[key].as_str().is_some_and(|text| !text.is_empty()), "{key}: {json}");
    }
    assert_eq!(words["sending"], "{name} भेजी जा रही है");
    assert!(out.contains("Profile photo"), "{out}");
}

// UI-GENUI-021: the module clones a row it is given; the page's words and icons are in it.
#[test]
fn a_template_holds_the_row_the_module_clones() {
    let out = html(photo());
    let template = out
        .split(r#"<template data-slot="upload-row">"#)
        .nth(1)
        .and_then(|rest| rest.split("</template>").next())
        .expect("a template");
    assert!(template.contains(r#"data-slot="upload-item""#), "{template}");
    assert!(template.contains(r#"data-slot="attachment""#), "{template}");
    assert!(template.contains("<progress"), "{template}");
    for hook in ["data-ui-cancel", "data-ui-retry", "data-ui-remove"] {
        assert!(template.contains(hook), "{hook}: {template}");
    }
    assert!(template.contains("Cancel") && template.contains("Try again") && template.contains("Remove"));
    // Each button's name ends with the file's, which the module writes into the hidden span.
    assert!(template.matches("data-ui-sr").count() >= 3, "{template}");
}

// UI-GENUI-021: files the server already has are rows of the list.
#[test]
fn files_already_stored_are_listed_as_attachments() {
    let out = html(
        photo().file(
            UploadedFile::new("Gurpreet-Singh-aadhaar.pdf")
                .size(240 * 1024)
                .href("/files/17"),
        ),
    );
    assert!(out.contains(r#"<li data-slot="upload-item" data-state="done""#), "{out}");
    assert!(out.contains(r#"data-slot="attachment-title" href="/files/17""#), "{out}");
    assert!(out.contains("Gurpreet-Singh-aadhaar.pdf"), "{out}");
    assert!(out.contains("240 KB"), "{out}");
    // A file with nothing to remove it has no button.
    let list = out.split(r#"<ul data-slot="upload-list""#).nth(1).and_then(|r| r.split("</ul>").next()).unwrap();
    assert!(!list.contains("<button"), "{list}");
}

// UI-GENUI-021: a stored file removes itself with a post through the page's one post form.
#[test]
fn a_removable_file_posts_through_the_pages_post_form_and_needs_the_core_runtime() {
    let upload = photo().file(UploadedFile::new("a.png").remove("/files/17/remove"));
    let out = html(upload.clone());
    assert!(out.contains(r#"form="ui-post" formaction="/files/17/remove" formmethod="post""#), "{out}");
    let page = View::new("x").body(upload);
    assert_eq!(page.uses(), Uses::UPLOAD.with(Uses::CORE));
    assert!(page.to_html().contains(r#"id="ui-post""#));
}

// UI-GENUI-021: hostile file names, labels and hints are text.
#[test]
fn hostile_text_is_escaped_everywhere() {
    let evil = "<img src=x onerror=alert(1)><script>alert(2)</script>";
    let out = html(
        Upload::new("f", evil)
            .action("/up")
            .hint(evil)
            .error(evil)
            .file(UploadedFile::new(evil).href("/ok")),
    );
    for bad in ["<img", "<script"] {
        assert!(!out.contains(bad), "{bad}: {out}");
    }
    assert!(out.contains("&lt;img src=x onerror=alert(1)&gt;"), "{out}");
    assert!(!out.contains(" style="), "{out}");
    assert!(!out.contains(" onerror=") || out.contains("&lt;img"), "{out}");
}

// UI-GENUI-021: states.
#[test]
fn disabled_invalid_required_and_compact_are_said_in_attributes() {
    let out = html(photo().disabled(true));
    assert!(out.contains("data-disabled") && out.contains(r#" disabled"#), "{out}");
    let out = html(photo().error("Choose a photo under 2 MB."));
    assert!(out.contains(r#"aria-invalid="true""#), "{out}");
    assert!(out.contains("data-invalid"), "{out}");
    assert!(out.contains(r#"<p data-slot="field-error" id="upload-photo-error">Choose a photo under 2 MB.</p>"#), "{out}");
    assert!(out.contains("upload-photo-error"), "described by the error: {out}");
    let out = html(photo().required());
    assert!(out.contains(" required"), "{out}");
    let out = html(photo().compact());
    assert!(out.contains("data-compact"), "{out}");
}

// UI-GENUI-021: what a page loads.
#[test]
fn a_page_with_an_upload_loads_the_upload_module_and_its_styles_only() {
    let page = View::new("x").body(photo());
    assert_eq!(page.uses(), Uses::UPLOAD);
    let html = page.to_html();
    assert!(html.contains(assets::UPLOAD_URL), "{html}");
    assert!(html.contains(assets::UPLOAD_STYLES_URL), "{html}");
    assert!(!html.contains(assets::CORE_URL), "{html}");
    assert!(!html.contains(assets::MENUS_URL) && !html.contains(assets::TABLE_URL), "{html}");
    let plain = View::new("x").body(navree_ui::el("p").child("Nothing to send")).to_html();
    assert!(!plain.contains(assets::UPLOAD_URL) && !plain.contains(assets::UPLOAD_STYLES_URL), "{plain}");
}

// UI-GENUI-021: the token reaches the plain form.
#[test]
fn the_session_token_goes_in_a_hidden_field_of_the_form() {
    let out = html(photo().csrf("tok\"><x>"));
    assert!(out.contains(r#"<input type="hidden" name="_csrf" value="tok&quot;&gt;&lt;x&gt;">"#), "{out}");
    let field = html(Upload::new("a", "A").csrf("tok"));
    assert!(!field.contains("_csrf"), "a field inside a form leaves the token to the form: {field}");
}

// UI-GENUI-021: the module reads the markup it keeps through a morph.
#[test]
fn the_module_may_mark_the_root_ready_and_a_morph_keeps_the_mark() {
    let out = html(photo());
    assert!(out.contains(r#"data-ui-keep-attrs="data-ready""#), "{out}");
}
