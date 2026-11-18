//! Page assembly for the board screen, linking the plugin's stylesheet.

use navree::ui::{AppPage, AppShell, SafeHtml};

use crate::BOARD_SHEET;

/// Wraps rendered board markup inside the app's `AppShell` and links [`BOARD_SHEET`].
pub fn board_page(shell: AppShell, title: &'static str, html: String) -> AppPage {
    shell
        .page(title)
        .stylesheet(&BOARD_SHEET)
        .body(SafeHtml::trusted(html))
}
