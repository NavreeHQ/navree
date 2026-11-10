//! Failure files and screenshots.

use std::path::Path;
use std::time::Duration;

use navree_browser::Browser;

use crate::browser_or_skip;
use crate::support::{Fixture, has_chrome};

/// The width and height in a PNG's header.
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[1..4], b"PNG", "{} is a PNG", path.display());
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (width, height)
}

#[tokio::test]
async fn a_failed_step_saves_a_screenshot_the_html_and_the_console_log() {
    if !has_chrome("a_failed_step_saves_a_screenshot_the_html_and_the_console_log") {
        return;
    }
    let site = Fixture::start().await;
    let artifacts = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let browser = Browser::builder()
        .artifacts(artifacts.path())
        .timeout(Duration::from_millis(400))
        .launch()
        .await
        .unwrap();
    let page = browser.visit(site.url("/broken")).await.unwrap();
    page.assert_see("Half a page").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let error = page.assert_see("Deal saved").await.unwrap_err();

    let dir = artifacts
        .path()
        .join("failures")
        .join("a_failed_step_saves_a_screenshot_the_html_and_the_console_log");
    assert_eq!(browser.artifacts_dir(), dir);
    let files = error.files().expect("failure files").clone();
    let screenshot = files.screenshot.expect("a screenshot");
    let html = files.html.expect("the html");
    let console = files.console.expect("the console log");
    assert_eq!(screenshot, dir.join("01-failure.png"));
    assert_eq!(html, dir.join("01-failure.html"));
    assert_eq!(console, dir.join("01-console.log"));
    assert_eq!(png_size(&screenshot), (1440, 900));
    let html = std::fs::read_to_string(&html).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("Half a page"), "{html}");
    let console = std::fs::read_to_string(&console).unwrap();
    for line in [
        "[log] starting up",
        "[error] the widget failed",
        "[exception] Error: kaboom",
        "/missing.json -> 404",
    ] {
        assert!(console.contains(line), "{line} in:\n{console}");
    }

    let message = error.to_string();
    assert!(
        message.starts_with("assert_see(\"Deal saved\") failed after"),
        "{message}"
    );
    assert!(
        message.contains("the text is not visible on http://127.0.0.1:"),
        "{message}"
    );
    assert!(message.contains("/broken\n  screenshot: "), "{message}");
    assert!(
        message.contains("01-failure.png\n  html:       "),
        "{message}"
    );
    assert!(
        message.contains("01-failure.html\n  console:    "),
        "{message}"
    );
    assert!(message.ends_with("01-console.log"), "{message}");
    assert_eq!(
        format!("{error:?}"),
        message,
        "Debug prints the same report"
    );

    // The next failure is numbered after it.
    let error = page.press("Nothing here").await.unwrap_err();
    assert!(
        error
            .files()
            .unwrap()
            .screenshot
            .as_ref()
            .unwrap()
            .ends_with("02-failure.png"),
        "{error}"
    );
}

#[tokio::test]
async fn screenshots_on_purpose_are_numbered_and_named() {
    if !has_chrome("screenshots_on_purpose_are_numbered_and_named") {
        return;
    }
    let site = Fixture::start().await;
    let artifacts = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let browser = Browser::builder()
        .artifacts(artifacts.path())
        .launch()
        .await
        .unwrap();
    let page = browser.visit(site.url("/tall")).await.unwrap();
    let shot = page.screenshot("board-after-move").await.unwrap();
    assert!(
        shot.ends_with("01-board-after-move.png"),
        "{}",
        shot.display()
    );
    assert_eq!(png_size(&shot), (1440, 900));
    let full = page.screenshot_full_page("whole page").await.unwrap();
    assert!(full.ends_with("02-whole-page.png"), "{}", full.display());
    let (width, height) = png_size(&full);
    assert_eq!(width, 1440);
    assert!(height >= 3000, "{height}");
}

#[tokio::test]
async fn failures_go_under_the_default_artifacts_folder() {
    let (browser, site) = browser_or_skip!("failures_go_under_the_default_artifacts_folder");
    let dir = browser.artifacts_dir().to_path_buf();
    assert!(
        dir.ends_with(
            Path::new("navree-browser")
                .join("failures")
                .join("failures_go_under_the_default_artifacts_folder")
        ),
        "{}",
        dir.display()
    );
    let page = browser.visit(site.url("/second")).await.unwrap();
    let error = page
        .with_timeout(Duration::from_millis(100))
        .assert_see("Not here")
        .await
        .unwrap_err();
    assert!(
        error
            .files()
            .unwrap()
            .screenshot
            .as_ref()
            .unwrap()
            .starts_with(&dir)
    );
}

#[tokio::test]
async fn a_failure_on_a_background_page_still_saves_its_screenshot() {
    if !has_chrome("a_failure_on_a_background_page_still_saves_its_screenshot") {
        return;
    }
    let site = Fixture::start().await;
    let artifacts = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let browser = Browser::builder()
        .artifacts(artifacts.path())
        .timeout(Duration::from_millis(300))
        .launch()
        .await
        .unwrap();
    let first = browser.visit(site.url("/second")).await.unwrap();
    // A second page in the same browser covers the first.
    let second = browser.visit(site.url("/second")).await.unwrap();

    // Chrome renders no frames for a page that is not in front, so the
    // capture used to wait out its whole timeout and give up.
    let started = std::time::Instant::now();
    let error = first.assert_see("Not here").await.unwrap_err();
    let screenshot = error
        .files()
        .and_then(|files| files.screenshot.clone())
        .expect("a screenshot of the background page");
    assert_eq!(png_size(&screenshot), (1440, 900));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the capture took {:?}",
        started.elapsed()
    );

    // The same for the newest page, and for an explicit screenshot of the
    // first one afterwards.
    let error = second.assert_see("Not here").await.unwrap_err();
    assert!(error.files().unwrap().screenshot.is_some());
    first.screenshot("again").await.unwrap();
}
