//! Markdown and code blocks on a page: what a page loads for them, what they write, and
//! how a real reply from a model reads. The hostile cases are in `security.rs`.

// UI-GENUI-024

use navree_ui::{CodeBlock, Markdown, MarkdownLabels, Uses, View, assets, render};

/// An answer as a model writes it: a heading, figures in a table, a list with links, and code.
const REPLY: &str = "## Weekly summary\n\nUPI payments from **Amritsar** grew *12%* this week.\n\n| City | Orders | Revenue |\n|---|---:|---:|\n| Ludhiana | 1,240 | ₹18,40,000 |\n| Jalandhar | 860 | ₹11,25,500 |\n\n1. Follow up with [Harleen Kaur](mailto:harleen.kaur@example.com)\n2. Read the [full report](/reports/weekly)\n3. See [the tax rules](https://www.incometax.gov.in/)\n\n- [x] Send the invoices\n- [ ] Call Gurpreet Singh\n\n```sql\nSELECT city, SUM(total) FROM orders GROUP BY city;\n```\n\nUse `ORDER BY` to sort.\n";

#[test]
fn a_page_with_markdown_links_its_stylesheet_and_ships_no_script() {
    let page = View::new("Reply").body(Markdown::new(REPLY));
    assert_eq!(page.uses(), Uses::MARKDOWN);
    let html = page.to_html();
    assert!(html.contains(assets::MARKDOWN_URL), "{html}");
    assert!(!html.contains("<script"), "{html}");
}

#[test]
fn a_page_without_markdown_links_no_markdown_stylesheet() {
    let page = View::new("Plain").body(navree_ui::Text::new("hello"));
    assert!(!page.to_html().contains(assets::MARKDOWN_URL));
}

#[test]
fn a_code_block_alone_links_the_same_stylesheet() {
    let page = View::new("Code").body(CodeBlock::new("rust", "let a = 1;"));
    assert_eq!(page.uses(), Uses::MARKDOWN);
    assert!(page.to_html().contains(assets::MARKDOWN_URL));
}

#[test]
fn a_copy_button_adds_the_core_runtime_and_nothing_else() {
    let page = View::new("Code").body(CodeBlock::new("sh", "cargo build").copy_button());
    assert!(page.uses().has(Uses::CORE));
    assert!(page.uses().has(Uses::MARKDOWN));
    assert!(!page.uses().has(Uses::MENUS) && !page.uses().has(Uses::OVERLAY));
    let html = page.to_html();
    assert!(html.contains("Copy code"), "{html}");
}

#[test]
fn a_models_answer_reads_as_a_document() {
    let html = Markdown::new(REPLY).heading_level(2).to_html();
    assert!(
        html.starts_with(r#"<div data-slot="markdown" data-untrusted>"#),
        "{html}"
    );
    assert!(html.contains("<h3>Weekly summary</h3>"), "{html}");
    assert!(html.contains("<strong>Amritsar</strong>"), "{html}");
    // The table scrolls in its own labelled box and its numbers line up at the end.
    assert!(html.contains(r#"data-slot="markdown-table""#), "{html}");
    assert!(
        html.contains(r#"<td data-cell-align="end">₹18,40,000</td>"#),
        "{html}"
    );
    // A mail link, a link on this site and a link to another site, each as it should be.
    assert!(
        html.contains(r#"href="mailto:harleen.kaur@example.com""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/reports/weekly">full report</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"href="https://www.incometax.gov.in/""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-host="www.incometax.gov.in""#),
        "{html}"
    );
    // Task items, a coloured code block and inline code.
    assert!(
        html.contains(r#"aria-label="Done" checked disabled"#),
        "{html}"
    );
    assert!(html.contains(r#"<span data-t="k">SELECT</span>"#), "{html}");
    assert!(html.contains(r#"<span data-t="f">SUM</span>"#), "{html}");
    assert!(
        html.contains(r#"<code data-slot="code">ORDER BY</code>"#),
        "{html}"
    );
}

#[test]
fn the_same_text_drawn_twice_is_the_same_markup() {
    let one = Markdown::new(REPLY).to_html();
    let two = Markdown::new(REPLY).to_html();
    assert_eq!(one, two);
}

#[test]
fn the_fragment_has_no_wrapper_and_the_page_element_has_one() {
    let fragment = Markdown::new("hi").to_fragment_html();
    assert_eq!(fragment, "<p>hi</p>");
    let wrapped = Markdown::new("hi").to_html();
    assert_eq!(
        wrapped,
        r#"<div data-slot="markdown" data-untrusted><p>hi</p></div>"#
    );
}

#[test]
fn an_empty_text_is_an_empty_element() {
    assert_eq!(
        Markdown::new("").to_html(),
        r#"<div data-slot="markdown" data-untrusted></div>"#
    );
}

#[test]
fn each_language_the_docs_use_is_coloured() {
    for (language, code, expect) in [
        ("rust", "let x = 1;", r#"<span data-t="k">let</span>"#),
        (
            "toml",
            "[server]\nport = 8080",
            r#"<span data-t="k">[server]</span>"#,
        ),
        (
            "sh",
            "cargo build --release",
            r#"<span data-t="t">--release</span>"#,
        ),
        (
            "ts",
            "const a: string = \"x\";",
            r#"<span data-t="k">const</span>"#,
        ),
        ("tsx", "<A b=\"c\" />", r#"data-t=""#),
        (
            "json",
            "{\"city\": \"Ludhiana\"}",
            r#"<span data-t="t">&quot;city&quot;</span>"#,
        ),
        ("sql", "SELECT 1", r#"<span data-t="k">SELECT</span>"#),
        (
            "http",
            "GET /deals HTTP/1.1",
            r#"<span data-t="k">GET</span>"#,
        ),
        (
            "html",
            "<a href=\"/x\">go</a>",
            r#"<span data-t="t">href</span>"#,
        ),
        (
            "css",
            "a { color: red; }",
            r#"<span data-t="t">color</span>"#,
        ),
        ("yaml", "name: web", r#"<span data-t="t">name</span>"#),
        (
            "env",
            "DATABASE_URL=x",
            r#"<span data-t="t">DATABASE_URL</span>"#,
        ),
    ] {
        let html = render(&CodeBlock::new(language, code).into());
        assert!(html.contains(expect), "{language}: {html}");
    }
}

#[test]
fn the_words_are_a_translation_away() {
    let labels = MarkdownLabels {
        copy_code: "ਕੋਡ ਕਾਪੀ ਕਰੋ".into(),
        ..MarkdownLabels::default()
    };
    let html = render(
        &CodeBlock::new("sh", "ls")
            .copy_button()
            .labels(labels)
            .into(),
    );
    assert!(html.contains("ਕੋਡ ਕਾਪੀ ਕਰੋ"), "{html}");
}
