//! Phones, tablets and other viewports.

use navree_browser::Viewport;

use crate::browser_or_skip;

#[tokio::test]
async fn a_phone_viewport_shows_the_phone_layout() {
    let (browser, site) = browser_or_skip!("a_phone_viewport_shows_the_phone_layout");
    let page = browser
        .visit_with(site.url("/mobile"), Viewport::phone())
        .await
        .unwrap();
    page.assert_see("width 390 ratio 3 touch true")
        .await
        .unwrap();
    page.assert_see("Android").await.unwrap();
    page.assert_see("Mobile Safari").await.unwrap();
    page.assert_missing("Companies").await.unwrap();
    page.press("Open menu").await.unwrap();
    page.assert_see("Companies").await.unwrap();

    page.resize(768, 1024).await.unwrap();
    page.assert_see("width 768 ratio 3 touch true")
        .await
        .unwrap();
    page.assert_missing("Open menu").await.unwrap();
}

#[tokio::test]
async fn tablet_and_desktop_presets() {
    let (browser, site) = browser_or_skip!("tablet_and_desktop_presets");
    let tablet = browser
        .visit_with(site.url("/mobile"), Viewport::tablet())
        .await
        .unwrap();
    tablet
        .assert_see("width 820 ratio 2 touch true")
        .await
        .unwrap();
    let desktop = browser
        .visit_with(site.url("/mobile"), Viewport::desktop())
        .await
        .unwrap();
    desktop
        .assert_see("width 1440 ratio 1 touch false")
        .await
        .unwrap();
    desktop.assert_dont_see("HeadlessChrome").await.unwrap();
    desktop.assert_see("Chrome/").await.unwrap();
    let custom = browser
        .visit_with(
            site.url("/mobile"),
            Viewport::new(500, 700).with_user_agent("NavreeBot/1.0"),
        )
        .await
        .unwrap();
    custom
        .assert_see("width 500 ratio 1 touch false agent NavreeBot/1.0")
        .await
        .unwrap();
    custom.assert_visible("Open menu").await.unwrap();
}
