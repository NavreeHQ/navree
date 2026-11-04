//! Upload: a drop area around a native file input, the list of files, and the limits.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("upload.rs");

const MB: u64 = 1024 * 1024;

pub fn all() -> Vec<Example> {
    vec![
        single!("upload", "thumb", "An upload", upload_thumb)
            .start()
            .height(150),
        single!("upload", "default", "A photo", upload_default)
            .start()
            .height(210),
        single!("upload", "multiple", "Several files", upload_multiple)
            .start()
            .height(210),
        gallery!("upload", "limits", "Limits, in words", upload_limits).medium(),
        single!("upload", "files", "Files the server already has", upload_files)
            .start()
            .height(300),
        single!("upload", "chunks", "Large files in chunks", upload_chunks)
            .start()
            .height(230),
        single!("upload", "field", "A field in your own form", upload_field)
            .start()
            .height(290),
        gallery!("upload", "states", "States", upload_states).medium(),
        single!("upload", "rtl", "Right to left", upload_rtl)
            .start()
            .height(210),
    ]
}

fn upload_thumb() -> Upload {
    Upload::new("photo", "Profile photo")
        .action("/profile/photo")
        .limits(UploadLimits::new(2 * MB).accept(["image/png", "image/jpeg"]))
        .compact()
}

fn upload_default() -> Upload {
    Upload::new("photo", "Profile photo")
        .action("/profile/photo")
        .limits(UploadLimits::new(2 * MB).accept(["image/png", "image/jpeg"]))
}

fn upload_multiple() -> Upload {
    Upload::new("documents", "Documents for the loan application")
        .action("/applications/ludhiana-4471/documents")
        .limits(
            UploadLimits::new(10 * MB)
                .max_files(5)
                .accept(["application/pdf", "image/*"]),
        )
}

fn upload_limits() -> Vec<(&'static str, Upload)> {
    vec![
        (
            "Two kinds of image",
            Upload::new("a", "Profile photo")
                .id("limits-a")
                .limits(UploadLimits::new(2 * MB).accept(["image/png", "image/jpeg"])),
        ),
        (
            "A few kinds, and a count",
            Upload::new("b", "GST invoices")
                .id("limits-b")
                .limits(
                    UploadLimits::new(10 * MB)
                        .max_files(5)
                        .accept(["application/pdf", "image/*", "text/csv"]),
                ),
        ),
        (
            "Any kind, a small size",
            Upload::new("c", "Note for the delivery boy")
                .id("limits-c")
                .limits(UploadLimits::new(500 * 1024)),
        ),
    ]
}

fn upload_files() -> Upload {
    Upload::new("documents", "Documents")
        .action("/applications/ludhiana-4471/documents")
        .limits(UploadLimits::new(10 * MB).max_files(5).accept(["application/pdf"]))
        .file(
            UploadedFile::new("Aadhaar-Gurpreet-Singh.pdf")
                .size(240 * 1024)
                .href("/files/17")
                .remove("/files/17/remove"),
        )
        .file(
            UploadedFile::new("Salary-slip-October.pdf")
                .size(1_800_000)
                .href("/files/18")
                .remove("/files/18/remove"),
        )
        .file(UploadedFile::new("PAN-card.pdf").size(96 * 1024).href("/files/19"))
}

fn upload_chunks() -> Upload {
    Upload::new("video", "Shop video")
        .action("/shops/doaba-traders/video")
        .limits(UploadLimits::new(500 * MB).accept(["video/*"]))
        .chunks("/uploads/chunks")
        .chunk_from(8 * MB)
        .refresh("shop-files")
}

fn upload_field() -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", "/applications")
        .attr("enctype", "multipart/form-data")
        .child(
            Stack::gap(4).children((
                Field::new("applicant", "Applicant").control(Input::text("applicant").value("Harleen Kaur")),
                Upload::new("papers", "Papers")
                    .limits(UploadLimits::new(5 * MB).max_files(3).accept(["application/pdf"])),
                Button::primary("Apply").submit(),
            )),
        )
}

fn upload_states() -> Vec<(&'static str, Upload)> {
    vec![
        (
            "Disabled",
            Upload::new("a", "Photo")
                .id("states-a")
                .action("/photo")
                .disabled(true),
        ),
        (
            "With an error",
            Upload::new("b", "Photo")
                .id("states-b")
                .action("/photo")
                .required()
                .error("Choose a photo under 2 MB."),
        ),
        (
            "Sent by hand",
            Upload::new("c", "Price list")
                .id("states-c")
                .action("/prices")
                .manual()
                .limits(UploadLimits::new(MB).accept(["text/csv"])),
        ),
    ]
}

fn upload_rtl() -> Element {
    el("div").attr("dir", "rtl").child(
        Upload::new("photo", "صورة الملف")
            .action("/profile/photo")
            .limits(UploadLimits::new(2 * MB).accept(["image/png", "image/jpeg"])),
    )
}
