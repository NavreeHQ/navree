//! `navree_ui::pages::ReturnTo`: the address a sign-in sends the user back to.
//!
//! Invariant: whatever is accepted is a path on this site, and an independent URL
//! parser (`url`, which follows the URL standard that browsers follow) resolves it
//! against a site to a URL on that same site, with the same scheme, host and port;
//! and once its dot segments are resolved its path is still one path of that site, never two
//! slashes. A sign-in link made from it reads back as the same address, and the sign-in page
//! (and anything below it, however it is spelled with escapes) is never the way back.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_ui::pages::ReturnTo;

/// Whether `address` names the page at `/sign-in` or below it, read the way a server reads it:
/// escapes decoded, a trailing slash ignored.
fn is_sign_in(address: &str) -> bool {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    let mut bytes = Vec::with_capacity(path.len());
    let raw = path.as_bytes();
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%'
            && let (Some(high), Some(low)) = (
                raw.get(at + 1).and_then(|b| (*b as char).to_digit(16)),
                raw.get(at + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            bytes.push((high * 16 + low) as u8);
            at += 3;
        } else {
            bytes.push(raw[at]);
            at += 1;
        }
    }
    let path = String::from_utf8_lossy(&bytes);
    let path = path.trim_end_matches('/');
    path == "/sign-in" || path.starts_with("/sign-in/")
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The address as given, and as a request carries it in `next`.
    let query = format!("next={}", text);
    for found in [ReturnTo::parse(text), ReturnTo::from_query(&query)]
        .into_iter()
        .flatten()
    {
        let address = found.as_str();
        assert!(address.starts_with('/'));
        assert!(!address.starts_with("//") && !address.starts_with("/\\"));
        assert!(address.len() <= navree_ui::pages::return_to::MOST);
        for base in ["https://site.example/", "http://site.example:8080/a/b/c?x#y"] {
            let base = url::Url::parse(base).unwrap();
            let joined = base.join(address).expect("an accepted address resolves");
            assert_eq!(joined.scheme(), base.scheme());
            assert_eq!(joined.host_str(), base.host_str());
            assert_eq!(joined.port(), base.port());
            assert!(joined.username().is_empty());
            // Resolved, the path is still one path of this site: not two slashes, which a later
            // hop could read as a host (`/a%3F/..//evil.example`).
            assert!(!joined.path().starts_with("//"), "{address:?} -> {joined}");
        }
        // The sign-in page is never the way back, spelled any way it can be.
        assert!(ReturnTo::parse_for(address, "/sign-in").is_some() == !is_sign_in(address));
        let link = found.sign_in_href("/sign-in");
        let (_, back) = link.as_str().split_once('?').expect("a query");
        assert_eq!(ReturnTo::from_query(back).as_ref(), Some(&found));
    }
});
