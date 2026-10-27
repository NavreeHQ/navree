//! search_docs and read_doc on the docs of the app's version.

use navree_navigator::NavigatorOptions;
use navree_navigator::docs::{DocPage, DocSection, DocsBundle, cache_file};
use serde_json::json;

use crate::support::{Fixture, call, refused, write};

#[tokio::test]
async fn the_apps_version_is_served_from_the_embedded_docs() {
    let fixture = Fixture::new("");
    let navigator = fixture.navigator();
    let answer = call(&navigator, "search_docs", json!({"query": "read-only SQL query_database"})).await;
    assert_eq!(answer["source"], "embedded");
    assert!(answer.get("note").is_none(), "the versions match: {answer}");
    let results = answer["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert!(
        results.iter().any(|r| r["slug"] == "ai/navigator"),
        "{results:?}"
    );
    assert!(results[0]["url"].as_str().unwrap().starts_with("https://navree.com/docs/"));

    let limited = call(&navigator, "search_docs", json!({"query": "migrations", "limit": 2})).await;
    assert_eq!(limited["results"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_page_is_read_as_markdown() {
    let fixture = Fixture::new("");
    let navigator = fixture.navigator();
    let page = call(&navigator, "read_doc", json!({"slug": "ai/navigator"})).await;
    let page = page.as_str().unwrap();
    assert!(page.starts_with("# Navigator\n"), "{page}");
    assert!(page.contains("## Security"), "{page}");
    assert!(!page.contains("<Tabs>"), "components are converted");

    let section = call(&navigator, "read_doc", json!({"slug": "/docs/ai/navigator/", "section": "options"})).await;
    let section = section.as_str().unwrap();
    assert!(section.contains("## Options") && !section.contains("## Security"), "{section}");

    let message = refused(&navigator, "read_doc", json!({"slug": "ai/navigatr"})).await;
    assert!(message.contains("no docs page"), "{message}");
    let message = refused(&navigator, "read_doc", json!({"slug": "ai/navigator", "section": "nope"})).await;
    assert!(message.contains("options"), "lists the sections: {message}");
}

#[tokio::test]
async fn another_version_comes_from_the_cache() {
    let fixture = Fixture::new("");
    write(
        &fixture.root().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"navree\"\nversion = \"0.0.1\"\n",
    );
    let bundle = DocsBundle {
        version: "0.0.1".to_owned(),
        pages: vec![DocPage {
            slug: "basics/old".to_owned(),
            title: "The old way".to_owned(),
            sections: vec![DocSection {
                heading: "Usage".to_owned(),
                anchor: "usage".to_owned(),
                markdown: "Call `legacy_widget()`.".to_owned(),
            }],
            ..DocPage::default()
        }],
    };
    let cache = cache_file(&fixture.root().join(".navree-home"), "0.0.1");
    write(&cache, &serde_json::to_string(&bundle).unwrap());
    let navigator = fixture.navigator();
    let answer = call(&navigator, "search_docs", json!({"query": "legacy widget"})).await;
    assert_eq!(answer["source"], "cache");
    assert_eq!(answer["version"], "0.0.1");
    assert_eq!(answer["results"][0]["slug"], "basics/old");
}

#[tokio::test]
async fn an_uncached_version_offline_falls_back_with_a_note() {
    let fixture = Fixture::new("");
    write(
        &fixture.root().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"navree\"\nversion = \"0.0.2\"\n",
    );
    let navigator = fixture.navigator_with(NavigatorOptions {
        path: fixture.root().to_path_buf(),
        docs_offline: true,
        ..NavigatorOptions::default()
    });
    let answer = call(&navigator, "search_docs", json!({"query": "routes"})).await;
    assert_eq!(answer["source"], "embedded");
    assert!(answer["note"].as_str().unwrap().contains("0.0.2"), "{answer}");
    let page = call(&navigator, "read_doc", json!({"slug": "ai/navigator"})).await;
    assert!(page.as_str().unwrap().starts_with("> The docs of Navree 0.0.2"));
}
