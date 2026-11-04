//! `navree_ui::pages::ReturnTo`: the address a sign-in sends the user back to.
//!
//! Invariant: whatever is accepted is a path on this site, and an independent URL
//! parser (`url`, which follows the URL standard that browsers follow) resolves it
//! against a site to a URL on that same site, with the same scheme, host and port;
//! a sign-in link made from it reads back as the same address.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_ui::pages::ReturnTo;

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
        }
        let link = found.sign_in_href("/sign-in");
        let (_, back) = link.as_str().split_once('?').expect("a query");
        assert_eq!(ReturnTo::from_query(back).as_ref(), Some(&found));
    }
});
