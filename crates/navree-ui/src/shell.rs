//! The flash cookie: a toast that survives a redirect when JavaScript is off.

use http::header::{HeaderValue, InvalidHeaderValue};

/// The cookie holding a toast for the next page.
pub(crate) const FLASH_COOKIE: &str = "navree_ui_flash";

/// A cookie that shows `text` on the next page.
pub(crate) fn flash_cookie(text: &str) -> Result<HeaderValue, InvalidHeaderValue> {
    let mut value = String::with_capacity(text.len() + 64);
    value.push_str(FLASH_COOKIE);
    value.push('=');
    for byte in text.bytes().take(512) {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => value.push(char::from(byte)),
            _ => {
                value.push('%');
                value.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
                value.push(char::from_digit(u32::from(byte & 15), 16).unwrap_or('0'));
            }
        }
    }
    value.push_str("; Path=/; Max-Age=30; SameSite=Lax; HttpOnly");
    HeaderValue::from_str(&value)
}
