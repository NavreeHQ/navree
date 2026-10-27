//! Actions: typing, selecting, checking, files, clicks, keys, drags,
//! scrolling and history.

use std::time::Duration;

use navree_browser::Target;

use crate::browser_or_skip;

#[tokio::test]
async fn type_into_clears_and_append_keeps() {
    let (browser, site) = browser_or_skip!("type_into_clears_and_append_keeps");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.type_into("Mail", "rep1@navree.test").await.unwrap();
    page.assert_value("Mail", "rep1@navree.test").await.unwrap();
    page.append("Memo", " back Monday.").await.unwrap();
    page.assert_value("Memo", "Call back Monday.").await.unwrap();
    page.type_into("Notes", "line one\nline two").await.unwrap();
    page.assert_value("Notes", "line one\nline two").await.unwrap();
    page.type_into("Name", "Zoë Ångström").await.unwrap();
    page.assert_value("Name", "Zoë Ångström").await.unwrap();
    page.type_into("Name", "").await.unwrap();
    page.assert_value("Name", "").await.unwrap();
    page.type_into("Due date", "2026-10-27").await.unwrap();
    page.assert_value("Due date", "2026-10-27").await.unwrap();
}

#[tokio::test]
async fn typing_into_a_read_only_field_fails() {
    let (browser, site) = browser_or_skip!("typing_into_a_read_only_field_fails");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let error = page
        .with_timeout(Duration::from_millis(300))
        .type_into("Read only", "x")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("is read-only"), "{error}");
}

#[tokio::test]
async fn select_by_text_or_value_and_custom_selects() {
    let (browser, site) = browser_or_skip!("select_by_text_or_value_and_custom_selects");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.select("Stage", "Negotiation").await.unwrap();
    page.assert_see("stage negotiation").await.unwrap();
    page.assert_value("Stage", "negotiation").await.unwrap();
    page.assert_value("Stage", "Negotiation").await.unwrap();
    page.select("Stage", "won").await.unwrap();
    page.assert_see("stage won").await.unwrap();

    let error = page
        .with_timeout(Duration::from_millis(300))
        .select("Stage", "Lost")
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains(r#"no option "Lost""#), "{message}");
    assert!(message.contains(r#""Negotiation""#), "{message}");

    page.select("Owner", "Grace Hopper").await.unwrap();
    page.assert_see_in("#owner", "Grace Hopper").await.unwrap();
}

#[tokio::test]
async fn check_and_uncheck_native_custom_and_hidden_controls() {
    let (browser, site) = browser_or_skip!("check_and_uncheck_native_custom_and_hidden_controls");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.check("Send me a summary").await.unwrap();
    page.assert_see("summary on").await.unwrap();
    page.check("Send me a summary").await.unwrap(); // already checked: no click
    page.assert_see("summary on").await.unwrap();
    page.uncheck("Send me a summary").await.unwrap();
    page.assert_see("summary off").await.unwrap();

    // A role=checkbox button with a <label for>.
    page.check("Accept terms").await.unwrap();
    let checked: String = page
        .script("return document.getElementById('terms').getAttribute('aria-checked')")
        .await
        .unwrap();
    assert_eq!(checked, "true");

    // A visually hidden input: its label is clicked.
    page.check("Dark mode").await.unwrap();
    let dark: bool = page
        .script("return document.getElementById('dark').checked")
        .await
        .unwrap();
    assert!(dark);

    page.check("Pro").await.unwrap();
    let error = page.uncheck("Pro").await.unwrap_err();
    assert!(error.to_string().contains("radio button"), "{error}");
    let error = page.check("Name").await.unwrap_err();
    assert!(error.to_string().contains("not a checkbox"), "{error}");
}

#[tokio::test]
async fn attach_puts_a_file_in_a_hidden_file_input() {
    let (browser, site) = browser_or_skip!("attach_puts_a_file_in_a_hidden_file_input");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("acme.png");
    std::fs::write(&logo, b"\x89PNG fake").unwrap();
    page.attach("Logo", &logo).await.unwrap();
    page.assert_see("acme.png").await.unwrap();

    let error = page.attach("Logo", dir.path().join("missing.png")).await.unwrap_err();
    assert!(error.to_string().contains("is not a file"), "{error}");
    let error = page
        .with_timeout(Duration::from_millis(300))
        .attach("Name", &logo)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not a file input"), "{error}");
}

#[tokio::test]
async fn press_waits_for_a_button_to_be_enabled_and_uncovered() {
    let (browser, site) = browser_or_skip!("press_waits_for_a_button_to_be_enabled_and_uncovered");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    // Disabled for 500 ms, and the whole page is covered for 400 ms.
    page.press("Later").await.unwrap();
    page.assert_see_in("@toast", "Later pressed").await.unwrap();
    page.click("#covered").await.unwrap();
    page.click("#covered").await.unwrap();
    page.assert_see("2 clicks").await.unwrap();
}

#[tokio::test]
async fn a_button_that_stays_covered_or_disabled_fails_with_the_reason() {
    let (browser, site) = browser_or_skip!("a_button_that_stays_covered_or_disabled_fails_with_the_reason");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.wait_until_missing("#overlay").await.unwrap(); // gone after 400 ms
    let quick = page.with_timeout(Duration::from_millis(150));
    let error = quick.press("Blocked").await.unwrap_err();
    assert!(
        error.to_string().contains(r#""Blocked" is covered by <span id="blocker">"#),
        "{error}"
    );
    let error = quick.press("Never").await.unwrap_err();
    assert!(error.to_string().contains(r#""Never" is disabled"#), "{error}");
    page.assert_enabled("Save").await.unwrap();
    let error = quick.assert_enabled("Never").await.unwrap_err();
    assert!(error.to_string().contains("is disabled"), "{error}");
}

#[tokio::test]
async fn keys_press_named_keys_and_combinations() {
    let (browser, site) = browser_or_skip!("keys_press_named_keys_and_combinations");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.type_into("Quick add", "Call Ada").await.unwrap();
    page.keys("Enter").await.unwrap();
    page.assert_see_in("#quick-list", "Call Ada").await.unwrap();
    page.assert_value("Quick add", "").await.unwrap();

    page.click("Command").await.unwrap();
    page.keys("Control+K").await.unwrap();
    page.assert_visible("Command palette").await.unwrap();
    page.keys("Escape").await.unwrap();
    page.assert_missing("Command palette").await.unwrap();

    let error = page.keys("Control+Nope").await.unwrap_err();
    assert!(error.to_string().contains("not a key name"), "{error}");
}

#[tokio::test]
async fn drag_moves_cards_with_pointer_events_and_native_drag_and_drop() {
    let (browser, site) = browser_or_skip!("drag_moves_cards_with_pointer_events_and_native_drag_and_drop");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let alpha = Target::from("@card").containing("Alpha renewal");
    page.drag(alpha.clone(), "[data-stage=won]").await.unwrap();
    page.assert_see_in("@toast", "Dropped in won").await.unwrap();
    page.assert_see_in("[data-stage=won]", "Alpha renewal").await.unwrap();
    page.within("[data-stage=won]")
        .await
        .unwrap()
        .assert_count("@card", 2)
        .await
        .unwrap();

    page.drag("Native card", "#native-to").await.unwrap();
    page.assert_see_in("@toast", "Native dropped").await.unwrap();
    page.assert_see_in("#native-to", "Native card").await.unwrap();
}

#[tokio::test]
async fn scroll_to_brings_an_element_into_view() {
    let (browser, site) = browser_or_skip!("scroll_to_brings_an_element_into_view");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    let before: f64 = page.script("return scrollY").await.unwrap();
    page.scroll_to("@load-more").await.unwrap();
    let after: f64 = page.script("return scrollY").await.unwrap();
    assert!(after > before + 1000.0, "{before} -> {after}");
    // Clicking scrolls by itself too.
    page.scroll_to("#title").await.unwrap();
    page.press("Load more").await.unwrap();
    page.assert_see("More loaded").await.unwrap();
}

#[tokio::test]
async fn confirm_dialogs_are_accepted() {
    let (browser, site) = browser_or_skip!("confirm_dialogs_are_accepted");
    let page = browser.visit(site.url("/playground")).await.unwrap();
    page.press("Delete").await.unwrap();
    page.assert_see_in("@toast", "Deleted").await.unwrap();
}

#[tokio::test]
async fn visit_back_and_refresh_wait_for_the_page() {
    let (browser, site) = browser_or_skip!("visit_back_and_refresh_wait_for_the_page");
    let page = browser.visit(site.url("/counter")).await.unwrap();
    page.assert_see("Loaded 1 times").await.unwrap();
    page.refresh().await.unwrap();
    page.assert_see("Loaded 2 times").await.unwrap();
    page.visit(site.url("/second")).await.unwrap();
    page.assert_title("Second page").await.unwrap();
    page.back().await.unwrap();
    page.assert_path_is("/counter").await.unwrap();
    page.assert_see("Loaded").await.unwrap();

    let fresh = browser.new_page().await.unwrap();
    let error = fresh.back().await.unwrap_err();
    assert!(error.to_string().contains("no earlier page"), "{error}");
}

#[tokio::test]
async fn visiting_an_unreachable_url_fails() {
    let (browser, _site) = browser_or_skip!("visiting_an_unreachable_url_fails");
    let error = browser.visit("http://127.0.0.1:1/").await.unwrap_err();
    let message = error.to_string();
    assert!(message.starts_with("visit(\"http://127.0.0.1:1/\") failed"), "{message}");
    assert!(message.contains("could not be loaded"), "{message}");
}
