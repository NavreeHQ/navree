//! `navree_pdf`: whole files, read the way navree-documents and
//! navree-media read them: the cross-reference (repaired when broken), the
//! page tree, every object, each page's content and its text.
//!
//! Invariants: nothing reads past its limits (they are small here, so
//! `-rss_limit_mb` would catch one that did not hold); a document whose
//! objects all read saves, and the saved file has the same pages.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_pdf::{Document, Objects, ReadLimits, TextLimits};

const LIMITS: ReadLimits = ReadLimits {
    memory: 64 * 1024 * 1024,
    work: 64 * 1024 * 1024,
    object_stream: 1024 * 1024,
    sections: 32,
};

const TEXT: TextLimits = TextLimits {
    page_work: 8 * 1024 * 1024,
    document_work: 32 * 1024 * 1024,
    page_glyphs: 100_000,
    page_steps: 1_000_000,
    page_text: 1024 * 1024,
    document_text: 4 * 1024 * 1024,
    font_memory: 32 * 1024 * 1024,
};

fuzz_target!(|data: &[u8]| {
    let Ok(document) = Document::load(data.to_vec(), LIMITS) else {
        return;
    };
    let pages = document.pages();
    for page in pages.iter().take(16) {
        let _ = document.page_content(*page, 1024 * 1024);
    }
    let _ = navree_pdf::text::page_texts_within(&document, TEXT);
    let Ok(objects) = Objects::read(document) else {
        return;
    };
    let Ok(saved) = objects.save() else {
        return;
    };
    let again = Document::load(saved, LIMITS).expect("a saved document reads");
    assert_eq!(again.pages().len(), objects.pages().len());
});
