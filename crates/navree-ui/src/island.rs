//! [`Island`]: a part of a server-drawn page that a script of the app's own draws in
//! the browser (a map, a rich editor, a chart the toolkit doesn't draw), while the rest
//! of the page stays the server's.
//!
//! # The contract
//!
//! The server writes an island as one element, with what shows before its script runs,
//! and with no script at all, inside it:
//!
//! ```html
//! <div data-slot="island" id="depots" data-ui-island="/_navree/ui/island/depots-1a2b3c4d5e6f.js"
//!      data-ui-props="{&quot;city&quot;:&quot;Ludhiana&quot;}" data-ui-keep-attrs="data-ui-mounted data-ui-failed">
//!   <div data-slot="island-body" data-ui-keep>the fallback</div>
//! </div>
//! ```
//!
//! The `islands` module (its own script, about 1 KB, loaded by a page with an island)
//! imports the island's script and calls its `mount(body, props, island)` export once:
//!
//! - `body` is the `island-body` element, the script's to draw in. A morph (a boosted
//!   link to the same page, a region's answer, a live swap) never touches it, so what
//!   the script drew stays, focus and all.
//! - `props` is the JSON the server wrote in `data-ui-props`, parsed: data, never markup
//!   or code.
//! - What `mount` returns (or resolves to) says the rest. A function is called when the
//!   island leaves: its element is gone from the page after a swap, or it now names
//!   another script. It undoes what `mount` did outside the body: listeners on the
//!   window, timers, sockets. An object can say more: its `update(props)` takes the
//!   props a morph brings, in place, and its `leave()` is called when the island leaves.
//!   An island whose script gives no `update` is left and mounted again with new props.
//! - Once mounted, the island carries `data-ui-mounted`. A script that fails to load or
//!   throws leaves `data-ui-failed`, and the body shows what the server drew.
//!
//! Only a script of the app's own runs: the module imports an address on this origin
//! under `/_navree/ui/island/`, named by its content hash, and nothing else, whatever a
//! page's markup says. The script is served `immutable` under the page's own content
//! security policy (`script-src 'self'`): no inline code and nothing from another site.
//! Its size is checked when the app compiles: [`IslandScript::new`] takes a ceiling.
//!
//! ```
//! use navree_ui::{Island, IslandScript, View, el};
//!
//! static DEPOTS: IslandScript =
//!     IslandScript::new("depots", b"export function mount(body, props) {}", 4 * 1024);
//!
//! #[derive(serde::Serialize)]
//! struct Depots {
//!     city: &'static str,
//! }
//!
//! let page = View::new("Depots").body(
//!     Island::new("depots", &DEPOTS)
//!         .label("Depots on a map")
//!         .props(&Depots { city: "Ludhiana" })
//!         .fallback(el("p").child("Ludhiana, Jalandhar and Amritsar")),
//! );
//! let html = page.to_html();
//! assert!(html.contains(navree_ui::assets::ISLANDS_URL));
//! assert!(html.contains(r#"data-ui-props="{&quot;city&quot;:&quot;Ludhiana&quot;}""#));
//! ```
//!
//! The app hands its scripts to the toolkit from one of its modules, so the toolkit
//! serves them: `m.contribute(DEPOTS)`.

use bytes::Bytes;
use http::HeaderMap;
use navree_http::response::{IntoResponse, Response};
use serde::Serialize;

use crate::assets;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el};

/// Where island scripts are served, under the toolkit's mount.
pub const ISLAND_PATH: &str = "/_navree/ui/island";

/// A script of the app's own that draws an island: a JavaScript module, compiled in with
/// `include_bytes!`, that exports `mount` (see [the module](self)). Contribute it from a
/// module (`m.contribute(SCRIPT)`) and the toolkit serves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IslandScript {
    name: &'static str,
    bytes: &'static [u8],
    hash: u64,
}

impl IslandScript {
    /// The script `bytes`, named `name` (1 to 64 lowercase letters, digits and `-`), at
    /// most `most` bytes long. A script past its ceiling, or a name of anything else,
    /// fails the app's build.
    pub const fn new(name: &'static str, bytes: &'static [u8], most: usize) -> Self {
        let letters = name.as_bytes();
        assert!(
            !letters.is_empty() && letters.len() <= 64,
            "an island's name has 1 to 64 characters"
        );
        let mut at = 0;
        while at < letters.len() {
            assert!(
                matches!(letters[at], b'a'..=b'z' | b'0'..=b'9' | b'-'),
                "an island's name has lowercase letters, digits and `-` only"
            );
            at += 1;
        }
        assert!(
            bytes.len() <= most,
            "an island's script is larger than its ceiling"
        );
        // FNV-1a, as `assets::content_hash` and the build of the toolkit's own files.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut at = 0;
        while at < bytes.len() {
            hash ^= bytes[at] as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
            at += 1;
        }
        Self { name, bytes, hash }
    }

    /// Its name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The script.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The first 12 hex digits of its content hash, as the toolkit's own files carry.
    pub fn hash(&self) -> String {
        let mut out = format!("{:016x}", self.hash);
        out.truncate(12);
        out
    }

    /// Where it is served: `/_navree/ui/island/{name}-{hash}.js`.
    pub fn url(&self) -> String {
        format!("{ISLAND_PATH}/{}-{}.js", self.name, self.hash())
    }
}

/// A part of a page that an [`IslandScript`] draws in the browser: see [the module](self)
/// for the contract. A page with one loads the `islands` module and no other script for
/// it; the island's own script comes when the page has loaded.
#[derive(Clone, Debug)]
#[must_use]
pub struct Island {
    el: Element,
    fallback: Node,
}

impl Island {
    /// An island named `id`, drawn by `script`.
    pub fn new(id: impl Into<AttrValue>, script: &IslandScript) -> Self {
        Self {
            el: el("div")
                .slot("island")
                .attr("id", id)
                .attr("data-ui-island", script.url())
                // What the browser says of the island stays through a morph from the server.
                .attr("data-ui-keep-attrs", "data-ui-mounted data-ui-failed")
                .uses(Uses::ISLAND),
            fallback: Node::Empty,
        }
    }

    /// The props the script mounts with: any value that serializes to JSON. They go in an
    /// attribute, escaped as every attribute is, and the browser reads them with
    /// `JSON.parse`, so a value can't become markup or code. A value that can't be
    /// written as JSON (a map with keys that aren't strings) is written as `null`.
    pub fn props(mut self, props: &impl Serialize) -> Self {
        let json = serde_json::to_string(props).unwrap_or_else(|_| "null".to_owned());
        self.el = self.el.attr("data-ui-props", json);
        self
    }

    /// What shows until the script has drawn the island, and with no script at all: a
    /// picture, the same numbers as a table, a link.
    pub fn fallback(mut self, nodes: impl IntoNodes) -> Self {
        self.fallback = Node::list(nodes);
        self
    }

    /// Names the island, which makes it a region a reader can move to.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.el = self
            .el
            .attr("role", "region")
            .attr("aria-label", label.into());
        self
    }
}

impl From<Island> for Node {
    fn from(island: Island) -> Self {
        island
            .el
            .child(
                el("div")
                    .slot("island-body")
                    .flag("data-ui-keep", true)
                    .child(island.fallback),
            )
            .into()
    }
}

/// An island script as the toolkit serves it: the script and its brotli form, made once.
#[derive(Clone, Debug)]
pub(crate) struct Served {
    script: IslandScript,
    hash: String,
    brotli: Bytes,
}

impl Served {
    /// The script, compressed for the requests to come.
    pub(crate) fn new(script: IslandScript) -> Self {
        Self {
            script,
            hash: script.hash(),
            brotli: Bytes::from(crate::compress::brotli(script.bytes)),
        }
    }
}

/// The scripts the app contributed, each name once: the first to take it keeps it.
pub(crate) fn served(scripts: impl IntoIterator<Item = IslandScript>) -> Vec<Served> {
    let mut out: Vec<Served> = Vec::new();
    for script in scripts {
        if !out.iter().any(|served| served.script.name == script.name) {
            out.push(Served::new(script));
        }
    }
    out
}

/// Answers `GET /_navree/ui/island/{file}`: the script named in `file`, `immutable` when
/// the hash is its own, uncached under an older hash (a page open across a deploy keeps
/// working), and not found for a name the app has no script for.
pub(crate) fn respond(file: &str, request: &HeaderMap, scripts: &[Served]) -> Response {
    let found = file
        .strip_suffix(".js")
        .and_then(|stem| stem.rsplit_once('-'))
        .and_then(|(name, hash)| {
            scripts
                .iter()
                .find(|served| served.script.name == name)
                .map(|served| (served, served.hash == hash))
        });
    let Some((served, current)) = found else {
        return http::StatusCode::NOT_FOUND.into_response();
    };
    assets::answer(
        &served.hash,
        "text/javascript; charset=utf-8",
        current,
        request,
        Bytes::from_static(served.script.bytes),
        None,
        Some(served.brotli.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, HeaderValue};

    static CLOCK: IslandScript = IslandScript::new(
        "clock",
        b"export function mount(body) { body.textContent = 'IST'; }",
        1024,
    );

    #[test]
    fn a_script_is_named_by_its_content_hash() {
        assert_eq!(CLOCK.hash(), assets::content_hash(CLOCK.bytes()));
        assert_eq!(
            CLOCK.url(),
            format!("/_navree/ui/island/clock-{}.js", CLOCK.hash())
        );
        assert!(CLOCK.url().starts_with(assets::MOUNT));
    }

    #[test]
    fn an_island_is_its_script_its_props_and_a_body_the_morph_keeps() {
        let html = crate::render(
            &Island::new("clock", &CLOCK)
                .props(&serde_json::json!({ "zone": "Asia/Kolkata" }))
                .fallback(el("p").child("10:30 IST"))
                .into(),
        );
        assert_eq!(
            html,
            format!(
                r#"<div data-slot="island" id="clock" data-ui-island="{}" data-ui-keep-attrs="data-ui-mounted data-ui-failed" data-ui-props="{{&quot;zone&quot;:&quot;Asia/Kolkata&quot;}}"><div data-slot="island-body" data-ui-keep><p>10:30 IST</p></div></div>"#,
                CLOCK.url()
            )
        );
    }

    #[test]
    fn the_script_is_served_by_its_hash_and_an_old_hash_still_gets_it_uncached() {
        let scripts = served([CLOCK, CLOCK]);
        assert_eq!(scripts.len(), 1, "one name, one script");
        let mut request = HeaderMap::new();
        request.insert(ACCEPT_ENCODING, HeaderValue::from_static("br"));
        let file = format!("clock-{}.js", CLOCK.hash());
        let current = respond(&file, &request, &scripts);
        assert_eq!(current.status(), http::StatusCode::OK);
        assert_eq!(
            current.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(current.headers()[CONTENT_ENCODING], "br");
        let old = respond("clock-000000000000.js", &HeaderMap::new(), &scripts);
        assert_eq!(old.status(), http::StatusCode::OK);
        assert_eq!(old.headers()[CACHE_CONTROL], "no-cache");
        for missing in [
            "map-000000000000.js",
            "clock.js",
            "../clock.js",
            "clock-x.css",
        ] {
            assert_eq!(
                respond(missing, &request, &scripts).status(),
                http::StatusCode::NOT_FOUND,
                "{missing}"
            );
        }
    }
}
