//! [`UiExtension`] and [`ExtensionFile`]: the script and the stylesheet of a component the
//! toolkit does not have, served by the toolkit and loaded on a page only where an element
//! names them.
//!
//! A plugin (or the app) builds each file when its crate compiles, so a file that is too
//! large or outside the rules fails the build and nothing is read at run time. It groups the
//! script and the stylesheet under a namespace and a name in a [`UiExtension`], and
//! contributes that from its module with `m.contribute(..)`. The toolkit serves every file
//! under `/_navree/ui/ext/`, named by the namespace, a dot, the name, a dash, the first 12 hex
//! digits of the file's content hash and `.js` or `.css`: `immutable` under the current hash,
//! uncached under an older one, so a page open across a deploy keeps working.
//!
//! An element names the extension it is the root of, and the page links the stylesheets and
//! preloads the scripts of the elements it holds. The browser side mounts each script once
//! and gives it only the routes the extension named.
//!
//! The namespace is the plugin's own (a lower-case letter followed by up to 63 lower-case
//! letters, digits or `_`); an app's own extensions use a namespace no plugin has, such as
//! `app`. A plugin contributing an extension outside its namespace stops the boot, and so
//! do two modules giving one extension name to different files.

/// Where extension files are served.
pub const EXTENSION_PATH: &str = "/_navree/ui/ext";

/// The browser contract every extension script is written for. Every 1.x release runs every
/// contract a 1.x release shipped; today that is contract 1.
pub const EXTENSION_CONTRACT: u32 = 1;

/// The most route prefixes an extension names, and the most attributes it keeps.
const MOST_NAMED: usize = 8;

/// Attributes an extension never keeps: the ones the runtime or the page acts on, a name or an
/// id that could stand in for one of the page's, and every attribute that holds an address.
/// (A name that starts with `data-ui-` or `on` is refused as well.)
const REFUSED_KEPT: [&str; 14] = [
    "style",
    "id",
    "name",
    "form",
    "href",
    "src",
    "action",
    "formaction",
    "poster",
    "cite",
    "ping",
    "data",
    "background",
    "xlink:href",
];

/// Whether two strings are the same, as a `const fn` can tell.
const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut at = 0;
    while at < a.len() {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether `text` starts with `start`.
const fn starts_with(text: &[u8], start: &[u8]) -> bool {
    if text.len() < start.len() {
        return false;
    }
    let mut at = 0;
    while at < start.len() {
        if text[at] != start[at] {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether `name` is 1 to 64 lower-case letters, digits and `-`: what an extension's name may
/// be.
const fn is_extension_name(name: &str) -> bool {
    let letters = name.as_bytes();
    if letters.is_empty() || letters.len() > 64 {
        return false;
    }
    let mut at = 0;
    while at < letters.len() {
        if !matches!(letters[at], b'a'..=b'z' | b'0'..=b'9' | b'-') {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether `hash` is 12 lower-case hex digits: what a served file's name carries.
const fn is_hash(hash: &str) -> bool {
    let digits = hash.as_bytes();
    if digits.len() != 12 {
        return false;
    }
    let mut at = 0;
    while at < digits.len() {
        if !matches!(digits[at], b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether `name` is an attribute an extension may keep: a lower-case letter followed by up to
/// 63 lower-case letters, digits or `-`, that is none of the attributes the runtime or the page
/// acts on.
const fn is_kept_attribute(name: &str) -> bool {
    let letters = name.as_bytes();
    if letters.is_empty() || letters.len() > 64 || !letters[0].is_ascii_lowercase() {
        return false;
    }
    let mut at = 1;
    while at < letters.len() {
        if !matches!(letters[at], b'a'..=b'z' | b'0'..=b'9' | b'-') {
            return false;
        }
        at += 1;
    }
    if starts_with(letters, b"data-ui-") || starts_with(letters, b"on") {
        return false;
    }
    let mut at = 0;
    while at < REFUSED_KEPT.len() {
        if same(name, REFUSED_KEPT[at]) {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether every attribute of `attributes` may be kept.
const fn all_kept(attributes: &[&str]) -> bool {
    let mut at = 0;
    while at < attributes.len() {
        if !is_kept_attribute(attributes[at]) {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether `prefix` is a route prefix an extension may name: it starts and ends with `/`, has
/// at least one segment (never `/` alone, never an empty segment), holds lower-case letters,
/// digits and `/ _ . -` only, has no `.` or `..` segment, and is not under `/_navree/`.
const fn is_route_prefix(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    let len = bytes.len();
    if len < 3 || bytes[0] != b'/' || bytes[len - 1] != b'/' {
        return false;
    }
    if starts_with(bytes, b"/_navree/") {
        return false;
    }
    // Each segment, from the byte after one `/` to the next.
    let mut start = 1;
    let mut at = 1;
    while at < len {
        let byte = bytes[at];
        if byte == b'/' {
            let segment = at - start;
            if segment == 0 {
                return false;
            }
            if segment == 1 && bytes[start] == b'.' {
                return false;
            }
            if segment == 2 && bytes[start] == b'.' && bytes[start + 1] == b'.' {
                return false;
            }
            start = at + 1;
        } else if !matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-') {
            return false;
        }
        at += 1;
    }
    true
}

/// Whether every prefix of `prefixes` may be named.
const fn all_routes(prefixes: &[&str]) -> bool {
    let mut at = 0;
    while at < prefixes.len() {
        if !is_route_prefix(prefixes[at]) {
            return false;
        }
        at += 1;
    }
    true
}

/// One file of an extension, read, checked and hashed when the plugin compiles: a script from
/// `extension_script!`, a stylesheet from `extension_sheet!` (both in
/// `navree_ui` with its `extension-files` feature, or `navree::ui` with the facade's
/// `ui-extension-files`). Give it to a [`UiExtension`]; nothing else builds one.
///
/// ```ignore
/// use navree_ui::{ExtensionFile, extension_script};
///
/// // The tests of plugins/geo prove this example: its script is read, hashed and checked
/// // against its ceiling of 200 KB when the crate compiles.
/// const MAP_SCRIPT: ExtensionFile = extension_script!("assets/map.js", 200 * 1024);
/// assert_eq!(MAP_SCRIPT.hash().len(), 12);
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ExtensionFile {
    bytes: &'static [u8],
    hash: &'static str,
    /// The namespace a stylesheet was scoped for; a script has none.
    namespace: Option<&'static str>,
}

impl ExtensionFile {
    /// What the file macros expand to. Not public API.
    #[doc(hidden)]
    pub const fn __new(
        bytes: &'static [u8],
        hash: &'static str,
        namespace: Option<&'static str>,
    ) -> ExtensionFile {
        assert!(
            is_hash(hash),
            "an extension file's hash is 12 lower-case hex digits"
        );
        ExtensionFile {
            bytes,
            hash,
            namespace,
        }
    }

    /// The file as it is served: a script as written, a stylesheet as scoped.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The 12 hex digits of its content hash.
    pub const fn hash(&self) -> &'static str {
        self.hash
    }
}

/// The namespace `file` was scoped for, when it is a stylesheet.
pub(crate) const fn file_namespace(file: &ExtensionFile) -> Option<&'static str> {
    file.namespace
}

/// An address built in a `const fn`: 168 bytes hold the longest (16 + 64 + 1 + 64 + 1 + 12 +
/// 4).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Address {
    bytes: [u8; 168],
    len: u8,
}

impl Address {
    const EMPTY: Address = Address {
        bytes: [0; 168],
        len: 0,
    };

    /// `/_navree/ui/ext/{namespace}.{name}-{hash}.{ext}`.
    const fn of(namespace: &str, name: &str, hash: &str, ext: &str) -> Address {
        let address = Address::EMPTY;
        let address = address.push(EXTENSION_PATH.as_bytes());
        let address = address.push(b"/");
        let address = address.push(namespace.as_bytes());
        let address = address.push(b".");
        let address = address.push(name.as_bytes());
        let address = address.push(b"-");
        let address = address.push(hash.as_bytes());
        let address = address.push(b".");
        address.push(ext.as_bytes())
    }

    /// This address with `text` after it.
    const fn push(mut self, text: &[u8]) -> Address {
        let mut at = 0;
        while at < text.len() {
            self.bytes[self.len as usize] = text[at];
            self.len += 1;
            at += 1;
        }
        self
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
}

/// A component's script, its stylesheet, or both, under a namespace and a name: served by
/// content hash and loaded on a page only where an element names them. Contribute it from the
/// module that ships it with `m.contribute(..)`, and name it from the component's root element
/// with `Element::extension`.
///
/// Build it in a `static`, from the files the two file macros read when the crate compiles. A
/// name, a namespace, a kept attribute or a route prefix outside the rules fails the build.
///
/// ```ignore
/// use navree_ui::{ExtensionFile, UiExtension, extension_script, extension_sheet};
///
/// // The tests of plugins/geo prove this example: the files are read, scoped and hashed when
/// // the plugin compiles, and the extension is contributed from its module.
/// const MAP_SCRIPT: ExtensionFile = extension_script!("assets/map.js", 200 * 1024);
/// const MAP_SHEET: ExtensionFile = extension_sheet!("assets/map.css", "geo", 32 * 1024);
///
/// static MAP: UiExtension = UiExtension::new("geo", "map")
///     .script(MAP_SCRIPT)
///     .sheet(MAP_SHEET)
///     .contract(1)
///     .keeps(&["data-moving"])
///     .routes(&["/geo/"]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UiExtension {
    namespace: &'static str,
    name: &'static str,
    script: Option<ExtensionFile>,
    sheet: Option<ExtensionFile>,
    contract: Option<u32>,
    keeps: &'static [&'static str],
    routes: &'static [&'static str],
    script_url: Address,
    sheet_url: Address,
}

impl UiExtension {
    /// An extension named `name` (1 to 64 lower-case letters, digits and `-`) under
    /// `namespace` (a lower-case letter followed by up to 63 lower-case letters, digits or
    /// `_`): a plugin's namespace, or one no plugin has, such as `app`, for the app's own. A
    /// name or a namespace outside those rules fails the build.
    pub const fn new(namespace: &'static str, name: &'static str) -> UiExtension {
        assert!(
            navree_kernel::__private::is_namespace(namespace),
            "a UI extension's namespace is a lower-case letter followed by up to 63 lower-case letters, digits or _"
        );
        assert!(
            is_extension_name(name),
            "a UI extension's name has 1 to 64 lower-case letters, digits and -"
        );
        UiExtension {
            namespace,
            name,
            script: None,
            sheet: None,
            contract: None,
            keeps: &[],
            routes: &[],
            script_url: Address::EMPTY,
            sheet_url: Address::EMPTY,
        }
    }

    /// The script, from `extension_script!`. It is a JavaScript module that exports
    /// `contract` and `mount`; see `contract` for which contract it is written for.
    pub const fn script(mut self, file: ExtensionFile) -> UiExtension {
        self.script_url = Address::of(self.namespace, self.name, file.hash, "js");
        self.script = Some(file);
        self
    }

    /// The stylesheet, from `extension_sheet!` for the same namespace, else the build fails.
    pub const fn sheet(mut self, file: ExtensionFile) -> UiExtension {
        let scoped_here = match file_namespace(&file) {
            Some(namespace) => same(namespace, self.namespace),
            None => false,
        };
        assert!(
            scoped_here,
            "a UI extension's stylesheet was scoped for another namespace: write extension_sheet!(.., \"<namespace>\", ..)"
        );
        self.sheet_url = Address::of(self.namespace, self.name, file.hash, "css");
        self.sheet = Some(file);
        self
    }

    /// The browser contract the script is written for (`1` today). A script with no contract
    /// stops the boot, and so does a contract the toolkit does not run.
    pub const fn contract(mut self, contract: u32) -> UiExtension {
        self.contract = Some(contract);
        self
    }

    /// The attributes a morph from the server leaves as the script set them, beside the ones
    /// the toolkit keeps (`data-ui-mounted` and `data-ui-failed`): at most 8, each a lower-case
    /// letter followed by up to 63 lower-case letters, digits or `-`, never `data-ui-*`,
    /// `style`, `on*`, `id`, `name`, `form` or an attribute that holds an address (`href`,
    /// `src`, `action`, `formaction`, `poster`, `cite`, `ping`, `data`, `background`,
    /// `xlink:href`). Anything else fails the build.
    pub const fn keeps(mut self, attributes: &'static [&'static str]) -> UiExtension {
        assert!(
            attributes.len() <= MOST_NAMED && all_kept(attributes),
            "a UI extension keeps only attributes of its own: at most 8, none of data-ui-*, style, on*, id, name, form or a URL attribute"
        );
        self.keeps = attributes;
        self
    }

    /// The route prefixes its script may call: at most 8, each starting and ending with `/`,
    /// with at least one segment (`/geo/`, never `/` alone), of lower-case letters, digits and
    /// `/ _ . -`, with no `.` or `..` segment, and never under `/_navree/`. The script may call
    /// a path under one of them, and the exact addresses the server wrote for its element
    /// (`Element::extension_call`), and nothing else. Anything outside the rules fails the
    /// build.
    pub const fn routes(mut self, prefixes: &'static [&'static str]) -> UiExtension {
        assert!(
            prefixes.len() <= MOST_NAMED && all_routes(prefixes),
            "a UI extension's routes are at most 8 paths of one segment or more that start and end with /, of lower-case letters, digits and / _ . -, with no . or .. segment, and not under /_navree/"
        );
        self.routes = prefixes;
        self
    }

    /// Its namespace.
    pub const fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Its name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Where its script is served: `/_navree/ui/ext/` and the namespace, a dot, the name, a
    /// dash, the first 12 hex digits of the script's content hash and `.js`. `None` when it has
    /// no script.
    pub fn script_url(&self) -> Option<&str> {
        self.script.map(|_| self.script_url.as_str())
    }

    /// Where its stylesheet is served: the same with `.css`. `None` when it has no stylesheet.
    pub fn sheet_url(&self) -> Option<&str> {
        self.sheet.map(|_| self.sheet_url.as_str())
    }

    /// Its script, when it has one.
    pub const fn script_file(&self) -> Option<ExtensionFile> {
        self.script
    }

    /// Its stylesheet, when it has one.
    pub const fn sheet_file(&self) -> Option<ExtensionFile> {
        self.sheet
    }

    /// The contract its script declares, when it declares one.
    pub const fn declared_contract(&self) -> Option<u32> {
        self.contract
    }

    /// The attributes it keeps through a morph, beside the toolkit's own.
    pub(crate) fn keeps_of(&self) -> &'static [&'static str] {
        self.keeps
    }

    /// The route prefixes its script may call.
    pub(crate) fn routes_of(&self) -> &'static [&'static str] {
        self.routes
    }

    /// The name it is served under before its hash: the namespace, a dot and the name
    /// (`geo.map`).
    pub(crate) fn stem(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }
}

/// An extension file's name as served, read apart: `{namespace}.{name}-{hash}.{ext}`.
struct FileName<'a> {
    namespace: &'a str,
    name: &'a str,
    hash: &'a str,
    ext: &'static str,
}

/// Reads `file`, the last part of an extension file's address, as the grammar writes it: a
/// namespace (a lower-case letter followed by up to 63 lower-case letters, digits or `_`), a
/// dot, a name (1 to 64 lower-case letters, digits and `-`), a dash, 12 lower-case hex digits
/// and `.js` or `.css`, with nothing before, between or after. `None` for anything else.
fn read_file_name(file: &str) -> Option<FileName<'_>> {
    let (rest, ext) = match file.strip_suffix(".js") {
        Some(rest) => (rest, "js"),
        None => (file.strip_suffix(".css")?, "css"),
    };
    let (stem, hash) = rest.rsplit_once('-')?;
    let (namespace, name) = stem.split_once('.')?;
    (is_hash(hash)
        && navree_kernel::__private::is_namespace(namespace)
        && is_extension_name(name))
    .then_some(FileName {
        namespace,
        name,
        hash,
        ext,
    })
}

/// Whether `value` is the address of a served extension script: `/_navree/ui/ext/` and a file
/// name the grammar takes with `.js`, nothing more (no query, no fragment, no other origin).
pub(crate) fn is_script_address(value: &str) -> bool {
    is_address(value, "js")
}

/// Whether `value` is the address of a served extension stylesheet.
pub(crate) fn is_sheet_address(value: &str) -> bool {
    is_address(value, "css")
}

fn is_address(value: &str, ext: &str) -> bool {
    value
        .strip_prefix(EXTENSION_PATH)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(read_file_name)
        .is_some_and(|file| file.ext == ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: ExtensionFile =
        ExtensionFile::__new(b"export const contract = 1;", "1a2b3c4d5e6f", None);
    const SHEET: ExtensionFile =
        ExtensionFile::__new(b"/* scoped for geo */", "0f9e8d7c6b5a", Some("geo"));
    const OTHER_SHEET: ExtensionFile =
        ExtensionFile::__new(b"/* scoped for another */", "0f9e8d7c6b5a", Some("other"));

    static MAP: UiExtension = UiExtension::new("geo", "map")
        .script(SCRIPT)
        .sheet(SHEET)
        .contract(1);
    static BADGE: UiExtension = UiExtension::new("geo", "badge").sheet(SHEET);

    /// A name that is no extension's is only a name: it compiles as a constant.
    const _: UiExtension = UiExtension::new("app", "rating");

    /// What the call panics with, as text.
    fn refusal(call: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let payload = std::panic::catch_unwind(call).expect_err("the call fails the build");
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .expect("a message")
    }

    /// A list that lives as long as the program, for a call that takes one.
    fn listed(items: Vec<&'static str>) -> &'static [&'static str] {
        items.leak()
    }

    #[test]
    fn an_extension_is_served_under_its_namespace_by_content_hash() {
        assert_eq!(
            MAP.script_url(),
            Some("/_navree/ui/ext/geo.map-1a2b3c4d5e6f.js")
        );
        assert_eq!(
            MAP.sheet_url(),
            Some("/_navree/ui/ext/geo.map-0f9e8d7c6b5a.css")
        );
        assert_eq!(MAP.declared_contract(), Some(1));
        assert_eq!(MAP.namespace(), "geo");
        assert_eq!(MAP.name(), "map");
        assert_eq!(MAP.stem(), "geo.map");
        assert_eq!(MAP.script_file().map(|file| file.hash()), Some("1a2b3c4d5e6f"));
        assert_eq!(MAP.sheet_file().map(|file| file.bytes()), Some(SHEET.bytes()));

        // A sheet-only extension has no script address, and no contract it was told.
        assert_eq!(BADGE.script_url(), None);
        assert_eq!(BADGE.script_file().map(|file| file.hash()), None);
        assert_eq!(
            BADGE.sheet_url(),
            Some("/_navree/ui/ext/geo.badge-0f9e8d7c6b5a.css")
        );
        assert_eq!(BADGE.declared_contract(), None);

        // The longest address the buffer holds.
        let namespace: &'static str = "a".repeat(64).leak();
        let name: &'static str = "b".repeat(64).leak();
        let longest = UiExtension::new(namespace, name).sheet(ExtensionFile::__new(
            b"x",
            "0f9e8d7c6b5a",
            Some(namespace),
        ));
        let url = longest.sheet_url().expect("a sheet");
        assert_eq!(url.len(), 162);
        assert!(is_sheet_address(url));

        // Two extensions with the same names and files are the same extension.
        let again = UiExtension::new("geo", "map")
            .script(SCRIPT)
            .sheet(SHEET)
            .contract(1);
        assert!(MAP == again);
        assert!(MAP != BADGE);

        // What a page's markup may name is what the grammar reads.
        assert!(is_script_address("/_navree/ui/ext/geo.map-1a2b3c4d5e6f.js"));
        assert!(is_sheet_address("/_navree/ui/ext/geo.map-0f9e8d7c6b5a.css"));
        assert!(is_script_address(
            "/_navree/ui/ext/notice_board.depot-map-0123456789ab.js"
        ));
        assert!(is_script_address(
            "/_navree/ui/ext/announcements.clock-0123456789ab.js"
        ));
        for refused in [
            // Two namespaces, a capital, a digit first, no namespace, a dot in the name, no hash.
            "/_navree/ui/ext/announcements.extra.clock-0123456789ab.js",
            "/_navree/ui/ext/Announcements.clock-0123456789ab.js",
            "/_navree/ui/ext/9lives.clock-0123456789ab.js",
            "/_navree/ui/ext/.clock-0123456789ab.js",
            "/_navree/ui/ext/announcements.clock.old-0123456789ab.js",
            "/_navree/ui/ext/announcements.clock.js",
            "/_navree/ui/ext/clock-0123456789ab.js",
            // A folder above, another origin, a query, a fragment, another folder.
            "/_navree/ui/ext/../core.js",
            "/_navree/ui/ext/../ext/geo.map-1a2b3c4d5e6f.js",
            "https://other.test/_navree/ui/ext/announcements.clock-0123456789ab.js",
            "//other.test/_navree/ui/ext/announcements.clock-0123456789ab.js",
            "/_navree/ui/ext/announcements.clock-0123456789ab.js?v=2",
            "/_navree/ui/ext/announcements.clock-0123456789ab.js#top",
            "/_navree/ui/island/announcements.clock-0123456789ab.js",
            "/_navree/ui/app/announcements.clock-0123456789ab.js",
            "_navree/ui/ext/announcements.clock-0123456789ab.js",
            "/_navree/ui/ext//announcements.clock-0123456789ab.js",
            "/_navree/ui/ext/",
            "",
            // A hash of capitals, of 11 digits, of 13, of letters past f.
            "/_navree/ui/ext/geo.map-1A2B3C4D5E6F.js",
            "/_navree/ui/ext/geo.map-1a2b3c4d5e6.js",
            "/_navree/ui/ext/geo.map-1a2b3c4d5e6f0.js",
            "/_navree/ui/ext/geo.map-1a2b3c4d5e6g.js",
            // A stylesheet is no script.
            "/_navree/ui/ext/geo.map-1a2b3c4d5e6f.css",
            "/_navree/ui/ext/geo.map-1a2b3c4d5e6f.mjs",
        ] {
            assert!(!is_script_address(refused), "{refused}");
        }
        assert!(!is_sheet_address("/_navree/ui/ext/geo.map-1a2b3c4d5e6f.js"));
        assert!(!is_sheet_address(
            "/_navree/ui/ext/geo.map-0f9e8d7c6b5a.css?v=2"
        ));
    }

    #[test]
    fn an_extension_name_or_namespace_outside_the_grammar_fails_the_build() {
        let namespace = "a lower-case letter followed by up to 63 lower-case letters, digits or _";
        let name = "1 to 64 lower-case letters, digits and -";
        let too_long: &'static str = "a".repeat(65).leak();
        for bad in ["", "Geo", "1geo", "geo-map", "geo map", too_long] {
            assert_eq!(
                refusal(move || {
                    let _ = UiExtension::new(bad, "map");
                }),
                format!("a UI extension's namespace is {namespace}"),
                "{bad:?}"
            );
        }
        for bad in ["", "Map", "map_view", "map.view", "map view", too_long] {
            assert_eq!(
                refusal(move || {
                    let _ = UiExtension::new("geo", bad);
                }),
                format!("a UI extension's name has {name}"),
                "{bad:?}"
            );
        }
        // The longest of each is taken, and so are digits and `_` after the first letter.
        let longest: &'static str = "a".repeat(64).leak();
        let _ = UiExtension::new(longest, longest);
        let _ = UiExtension::new("notice_board2", "depot-map-3");

        // A sheet scoped for another namespace, or for none.
        let scoped = "a UI extension's stylesheet was scoped for another namespace: write extension_sheet!(.., \"<namespace>\", ..)";
        assert_eq!(
            refusal(|| {
                let _ = UiExtension::new("geo", "map").sheet(OTHER_SHEET);
            }),
            scoped
        );
        assert_eq!(
            refusal(|| {
                let _ = UiExtension::new("geo", "map").sheet(SCRIPT);
            }),
            scoped
        );
    }

    #[test]
    fn an_extension_keeps_no_hook_id_name_form_or_url_attribute() {
        let keeps = "a UI extension keeps only attributes of its own: at most 8, none of data-ui-*, style, on*, id, name, form or a URL attribute";
        let too_long: &'static str = "a".repeat(65).leak();
        for bad in [
            "data-ui-click",
            "data-ui-x",
            "style",
            "onclick",
            "on",
            "id",
            "name",
            "form",
            "href",
            "src",
            "action",
            "formaction",
            "poster",
            "cite",
            "ping",
            "data",
            "background",
            "xlink:href",
            "Data-open",
            "1x",
            "data open",
            "",
            too_long,
        ] {
            let list = listed(vec!["data-moving", bad]);
            assert_eq!(
                refusal(move || {
                    let _ = UiExtension::new("geo", "map").keeps(list);
                }),
                keeps,
                "{bad:?}"
            );
        }
        // Nine are too many, whatever they are.
        let nine = listed(vec![
            "data-a", "data-b", "data-c", "data-d", "data-e", "data-f", "data-g", "data-h",
            "data-i",
        ]);
        assert_eq!(
            refusal(move || {
                let _ = UiExtension::new("geo", "map").keeps(nine);
            }),
            keeps
        );
        // A name that only looks like a refused one is taken.
        let taken = UiExtension::new("geo", "map").keeps(&["data-moving", "data-open"]);
        assert_eq!(taken.keeps_of(), ["data-moving", "data-open"]);
        let alike = UiExtension::new("geo", "map").keeps(&["data-on", "ident", "datatype", "x"]);
        assert_eq!(alike.keeps_of().len(), 4);
        let eight = UiExtension::new("geo", "map").keeps(&[
            "data-a", "data-b", "data-c", "data-d", "data-e", "data-f", "data-g", "data-h",
        ]);
        assert_eq!(eight.keeps_of().len(), 8);
        assert!(UiExtension::new("geo", "map").keeps_of().is_empty());
    }

    #[test]
    fn a_prefix_of_slash_alone_or_a_dot_segment_fails_the_build() {
        let routes = "a UI extension's routes are at most 8 paths of one segment or more that start and end with /, of lower-case letters, digits and / _ . -, with no . or .. segment, and not under /_navree/";
        for bad in [
            "/",
            "//",
            "/geo/../",
            "/geo/./",
            "/../",
            "/./",
            "geo/",
            "/geo",
            "/geo//",
            "//geo/",
            "/Geo/",
            "/geo map/",
            "/geo?x=1/",
            "/geo%2e/",
            "/_navree/",
            "/_navree/x/",
            "",
        ] {
            let list = listed(vec!["/geo/", bad]);
            assert_eq!(
                refusal(move || {
                    let _ = UiExtension::new("geo", "map").routes(list);
                }),
                routes,
                "{bad:?}"
            );
        }
        let nine = listed(vec![
            "/a/", "/b/", "/c/", "/d/", "/e/", "/f/", "/g/", "/h/", "/i/",
        ]);
        assert_eq!(
            refusal(move || {
                let _ = UiExtension::new("geo", "map").routes(nine);
            }),
            routes
        );
        // A name that only looks like a dot segment, a dot inside a segment and a dash are fine.
        let taken = UiExtension::new("geo", "map").routes(&["/geo/", "/geo/tiles/"]);
        assert_eq!(taken.routes_of(), ["/geo/", "/geo/tiles/"]);
        let alike = UiExtension::new("geo", "map").routes(&["/geo-v2/", "/a.b/", "/x_y/", "/9/"]);
        assert_eq!(alike.routes_of().len(), 4);
        assert!(UiExtension::new("geo", "map").routes_of().is_empty());
    }

    #[test]
    fn a_file_hash_that_is_not_twelve_hex_digits_fails_the_build() {
        for bad in ["", "1a2b3c4d5e6", "1a2b3c4d5e6f0", "1A2B3C4D5E6F", "1a2b3c4d5e6g"] {
            assert_eq!(
                refusal(move || {
                    let _ = ExtensionFile::__new(b"x", bad, None);
                }),
                "an extension file's hash is 12 lower-case hex digits",
                "{bad:?}"
            );
        }
    }
}
