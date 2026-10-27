//! Finding elements: CSS, test ids, labels, button text and text.

use navree_browser::Target;

use crate::browser_or_skip;

#[tokio::test]
async fn fields_are_found_by_label_aria_label_placeholder_and_name() {
    let (browser, site) =
        browser_or_skip!("fields_are_found_by_label_aria_label_placeholder_and_name");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    for (target, css) in [
        ("Name", "#name"),                                  // <label for>
        ("Notes", "[name=notes]"),                          // a wrapping <label>
        ("Search", "[aria-label=Search]"),                  // aria-label
        ("Email address", "[placeholder='Email address']"), // placeholder
        ("city", "[name=city]"),                            // name
        ("Nickname", "[aria-labelledby=by]"),               // aria-labelledby
    ] {
        page.type_into(target, format!("typed into {target}"))
            .await
            .unwrap();
        assert_eq!(
            page.value(format!("css={css}")).await.unwrap(),
            format!("typed into {target}"),
            "{target}"
        );
    }
}

#[tokio::test]
async fn buttons_and_links_are_found_by_text_or_accessible_name() {
    let (browser, site) =
        browser_or_skip!("buttons_and_links_are_found_by_text_or_accessible_name");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.press("Close dialog").await.unwrap(); // aria-label
    page.assert_see_in("@toast", "Closed").await.unwrap();
    page.press("Submit form").await.unwrap(); // <input type=submit value>
    page.assert_see_in("@toast", "Form submitted")
        .await
        .unwrap();
    page.press("Shout").await.unwrap(); // shown upper-case by CSS
    page.assert_see_in("@toast", "Shouted").await.unwrap();
    page.press("Next page").await.unwrap(); // a link
    page.assert_path_is("/second").await.unwrap();
}

#[tokio::test]
async fn css_selectors_and_test_ids() {
    let (browser, site) = browser_or_skip!("css_selectors_and_test_ids");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    assert_eq!(page.text("#title").await.unwrap(), "Playground");
    assert_eq!(page.text("css=section h1, h1").await.unwrap(), "Playground");
    assert_eq!(page.value("[name=city]").await.unwrap(), "");
    assert_eq!(page.text(".card").await.unwrap(), "Alpha renewal Edit");
    page.assert_count("@card", 3).await.unwrap(); // the hidden card does not count
    page.assert_count("@column", 2).await.unwrap();
}

#[tokio::test]
async fn text_targets_find_any_element_with_that_text() {
    let (browser, site) = browser_or_skip!("text_targets_find_any_element_with_that_text");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.assert_visible("Playground").await.unwrap();
    page.assert_visible("Loaded later").await.unwrap();
    // Exact match after collapsing whitespace, case included.
    page.assert_missing("playground").await.unwrap();
    page.assert_missing("Loaded").await.unwrap();
    // Hidden elements never match.
    page.assert_missing("Secret text").await.unwrap();
    page.assert_missing("Invisible text").await.unwrap();
}

#[tokio::test]
async fn containing_nth_and_within_narrow_a_target() {
    let (browser, site) = browser_or_skip!("containing_nth_and_within_narrow_a_target");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let gamma = Target::from("@card").containing("Gamma");
    assert_eq!(
        page.text(gamma.clone()).await.unwrap(),
        "Gamma renewal Edit"
    );
    assert_eq!(
        page.text(Target::from("@card").nth(1)).await.unwrap(),
        "Beta launch Edit"
    );
    page.assert_count(Target::from("@card").containing("renewal"), 2)
        .await
        .unwrap();

    // `within` searches inside one element; actions use its first match.
    let card = page.within(gamma).await.unwrap();
    card.press("Edit").await.unwrap();
    page.assert_see_in("@toast", "Editing Gamma renewal")
        .await
        .unwrap();
    assert_eq!(card.inner_text().await.unwrap(), "Gamma renewal Edit");

    let lead = page.within("[data-stage=lead]").await.unwrap();
    lead.assert_count("@card", 2).await.unwrap();
    lead.assert_see("Beta launch").await.unwrap();
    lead.assert_dont_see("Gamma renewal").await.unwrap();
    let alpha = lead.within("@card").await.unwrap();
    alpha.assert_see("Alpha renewal").await.unwrap();
}

#[tokio::test]
async fn all_returns_each_visible_match() {
    let (browser, site) = browser_or_skip!("all_returns_each_visible_match");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let cards = page.all("@card").await.unwrap();
    assert_eq!(cards.len(), 3);
    let mut texts = Vec::new();
    for card in &cards {
        texts.push(card.inner_text().await.unwrap());
    }
    assert_eq!(
        texts,
        [
            "Alpha renewal Edit",
            "Beta launch Edit",
            "Gamma renewal Edit"
        ]
    );
    cards[1].press("Edit").await.unwrap();
    page.assert_see_in("@toast", "Editing Beta launch")
        .await
        .unwrap();
    assert!(page.all("@nothing").await.unwrap().is_empty());
    assert_eq!(
        page.all(Target::from("@card").nth(2)).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn an_invalid_selector_fails_at_once() {
    let (browser, site) = browser_or_skip!("an_invalid_selector_fails_at_once");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let started = std::time::Instant::now();
    let error = page.click("[data-x=").await.unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(
        error.to_string().contains("not a valid CSS selector"),
        "{error}"
    );
}
