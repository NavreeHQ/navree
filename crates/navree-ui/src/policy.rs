//! [`PagePolicy`]: the content security policy the toolkit serves its pages with, and the one
//! way it can be widened.
//!
//! Every page loads scripts, styles and fonts from its own origin only, and nothing else:
//! [`PAGE_CSP`]. A component from a plugin that draws a map, plays a clip or
//! reads a live feed needs a few sources beyond that, and the app decides whether it gets them:
//! only the `ui.policy` section of the app's configuration widens the policy, never a plugin.
//!
//! ```toml
//! [ui.policy]
//! # wasm = true                              # 'wasm-unsafe-eval' on every page but the fresh ones
//!
//! [ui.policy.allow.geo]                      # a plugin's namespace, or app for the app's own
//! img_src = ["https://tile.openstreetmap.org"]
//! ```
//!
//! A module asks for what a plugin needs with [`PageSources`], so the boot can say, in its log,
//! what each plugin asked for and what the app allowed. A source the app has not allowed stays
//! refused by the browser, and the component that needs it shows what it shows without it.
//!
//! - Only four directives take sources: [`Directive::ImgSrc`], [`Directive::MediaSrc`],
//!   [`Directive::ConnectSrc`] and [`Directive::FontSrc`]. The sources of scripts, styles,
//!   workers and frames cannot be allowed, whatever a configuration or a plugin says, and a key
//!   for one stops the boot.
//! - A source is one of a short list of forms, written in lower case: `'self'`, `data:`,
//!   `blob:`, an https origin (a host, a leading `*.` for every host below a name, and a port),
//!   a wss origin for requests, and `http://localhost` or `http://127.0.0.1` with an optional
//!   port outside production and staging. A path, `*`, a bare scheme and every keyword that
//!   unlocks code (`'unsafe-inline'`, `'unsafe-eval'`, `'strict-dynamic'`, a nonce, a hash)
//!   are refused, and a source outside the grammar stops the boot, naming its key.
//! - WebAssembly is one switch for the whole app (`wasm`), which adds `'wasm-unsafe-eval'` to
//!   the scripts' own sources and nothing else.
//!
//! # Where the policy applies
//!
//! To every page and fragment the toolkit answers with its own policy: a [`View`](crate::View),
//! the answer to a region's fetch ([`region_answer`](crate::pages::region_answer)), a lazy list's
//! page, and the toolkit's error pages. The error pages get the composed policy from the toolkit's
//! error renderer, because the app's pipeline draws an error outside every layer of the app, so no
//! layer ever sees one. A response that sets a policy of its own (a picture of the app's, a route
//! that writes its own header) keeps it, and so does a page that starts clean (below). With
//! nothing allowed in `ui.policy`, nothing is touched: every response keeps the toolkit's policy
//! exactly.
//!
//! # Pages that start clean
//!
//! A page that takes a password, a code or a PIN must not share a document with a plugin's code.
//! [`View::fresh`](crate::View::fresh) and [`AppPage::fresh`](crate::AppPage::fresh) mark such a
//! page: in an app with a UI extension, a plugin's island script, or a policy the app widened,
//! it is entered and left by a full page load, no answer it gives is drawn into a page that is not
//! fresh, it keeps the toolkit's own policy whatever the app allows, and no extension mounts on
//! it. In any other app the mark changes nothing. The boot names, in its log and once for each
//! route, a page that holds a password field and is not fresh, and a reply that sends one as
//! operations; a password field inside markup written as a string is not seen, only the elements
//! of a page's tree and of a reply's operations are read.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use http::header::{CONTENT_SECURITY_POLICY, HeaderName, HeaderValue};
use navree_config::{Config, ConfigSection};
use navree_http::RouteMeta;
use navree_http::extract::Request;
use navree_http::middleware::Next;
use navree_http::response::Response;
use navree_kernel::{AppContext, BoxError};
use serde::Deserialize;

use crate::node::{AttrValue, Element, Node};
use crate::view::PAGE_CSP;

/// The most sources one directive of one key lists.
const MOST_SOURCES: usize = 32;

/// The namespace of the app's own modules: what a module that names no plugin asks for, and the
/// key of `ui.policy.allow` for the app's own sources.
const APP: &str = "app";

/// The `ui.policy` section of the configuration: what the app allows its pages to load beyond
/// their own origin. Nothing is allowed until it says so, and only four kinds of source can be
/// allowed (see [`Directive`]): never a script, a style, a worker or a frame.
///
/// A source that is not one the policy takes, a key that is not a plugin's namespace or `app`,
/// more than 32 sources for one directive, and a key under a namespace that is not one of the
/// four directives, stop the boot, and the message names the key.
///
/// ```toml
/// [ui.policy]
/// wasm = false
///
/// [ui.policy.allow.geo]
/// img_src = ["https://tile.openstreetmap.org"]
/// ```
///
/// ```
/// use navree_ui::PagePolicyOptions;
///
/// let options = PagePolicyOptions::default();
/// assert!(!options.wasm && options.allow.is_empty());
/// ```
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "ui.policy", docs = "/docs/ui/page-policy/", validate = Self::check)]
#[serde(default)]
#[non_exhaustive]
pub struct PagePolicyOptions {
    /// WebAssembly on every page but the fresh ones, for every script ('wasm-unsafe-eval'). Off unless set.
    pub wasm: bool,
    /// The sources each plugin may load, by the plugin's namespace, or `app` for the app's own.
    pub allow: BTreeMap<String, PolicySources>,
}

/// The sources one key of `ui.policy.allow` allows, by directive.
///
/// ```
/// use navree_ui::PolicySources;
///
/// let mut sources = PolicySources::default();
/// sources.img_src.push("https://tile.openstreetmap.org".to_owned());
/// assert!(sources.media_src.is_empty());
/// ```
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct PolicySources {
    /// Where pictures may come from.
    pub img_src: Vec<String>,
    /// Where audio and video may come from.
    pub media_src: Vec<String>,
    /// Where scripts may send requests to and open sockets with.
    pub connect_src: Vec<String>,
    /// Where fonts may come from.
    pub font_src: Vec<String>,
}

impl PolicySources {
    /// The sources listed for `directive`.
    fn of(&self, directive: Directive) -> &[String] {
        match directive {
            Directive::ImgSrc => &self.img_src,
            Directive::MediaSrc => &self.media_src,
            Directive::ConnectSrc => &self.connect_src,
            Directive::FontSrc => &self.font_src,
        }
    }
}

impl PagePolicyOptions {
    /// Checks the keys of `allow` and the sources under them (the boot calls this): every key is a
    /// plugin's namespace or `app`, every source is one the policy takes, and no directive lists
    /// more than 32. Each problem is a line of its own, names its key and never repeats a value.
    fn check(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        for (key, sources) in &self.allow {
            if !navree_kernel::__private::is_namespace(key) {
                problems.push(format!(
                    "ui.policy.allow.{key}: `{key}` is neither a namespace nor app"
                ));
            }
            for directive in Directive::ALL.iter().copied() {
                let listed = sources.of(directive);
                let name = directive.key();
                if listed.len() > MOST_SOURCES {
                    problems.push(format!(
                        "ui.policy.allow.{key}.{name} lists more than {MOST_SOURCES} sources"
                    ));
                }
                for (at, source) in listed.iter().enumerate() {
                    if let Some(forms) = source_problem(directive, source, false) {
                        problems.push(format!(
                            "ui.policy.allow.{key}.{name}[{at}] is not a source the page policy takes for {name}: {forms}"
                        ));
                    }
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }
}

/// What a source is for: the four kinds of load a plugin's component may need from beyond the
/// page's own origin. Scripts, styles, workers and frames are not among them: a page's code
/// comes from the app alone.
///
/// ```
/// use navree_ui::Directive;
///
/// assert_eq!(Directive::ImgSrc.as_str(), "img-src");
/// assert_eq!(Directive::ImgSrc.key(), "img_src");
/// assert_eq!(Directive::ALL.len(), 4);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Directive {
    /// Pictures (`img-src`).
    ImgSrc,
    /// Audio and video (`media-src`).
    MediaSrc,
    /// Requests and sockets a script opens (`connect-src`).
    ConnectSrc,
    /// Fonts (`font-src`).
    FontSrc,
}

impl Directive {
    /// The four, in this order.
    pub const ALL: &'static [Directive] = &[
        Directive::ImgSrc,
        Directive::MediaSrc,
        Directive::ConnectSrc,
        Directive::FontSrc,
    ];

    /// The directive as the policy header writes it: `img-src`, `media-src`, `connect-src` or
    /// `font-src`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImgSrc => "img-src",
            Self::MediaSrc => "media-src",
            Self::ConnectSrc => "connect-src",
            Self::FontSrc => "font-src",
        }
    }

    /// The directive as a key of `ui.policy.allow`: `img_src`, `media_src`, `connect_src` or
    /// `font_src`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::ImgSrc => "img_src",
            Self::MediaSrc => "media_src",
            Self::ConnectSrc => "connect_src",
            Self::FontSrc => "font_src",
        }
    }

    /// What a source of this directive can be, as the boot says it.
    const fn forms(self) -> &'static str {
        match self {
            Self::ImgSrc | Self::MediaSrc => {
                "'self', data:, blob: or an https origin such as https://tile.openstreetmap.org"
            }
            Self::ConnectSrc => "'self', an https origin or a wss origin",
            Self::FontSrc => "'self', data: or an https origin",
        }
    }
}

/// What a module asks the app to allow for a plugin's components: the sources they load from
/// and whether they need WebAssembly. Contribute it from the plugin's module
/// (`m.contribute(PageSources::new()...)`). Asking allows nothing: the app's `ui.policy` section
/// decides, and the boot says in its log what was asked for and what was allowed.
///
/// A source that is not one the policy takes (see [`Directive`]) stops the boot, naming the
/// plugin that asked for it.
///
/// ```
/// use navree_ui::{Directive, PageSources};
///
/// let sources = PageSources::new()
///     .ask(Directive::ImgSrc, "https://tile.openstreetmap.org", "map tiles")
///     .wasm("barcode decoder");
/// assert_eq!(sources.asks().count(), 1);
/// ```
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct PageSources {
    asks: Vec<(Directive, &'static str, &'static str)>,
    wasm: Option<&'static str>,
}

impl PageSources {
    /// Asks for nothing yet.
    pub fn new() -> PageSources {
        PageSources::default()
    }

    /// Asks for `source` in `directive`, and says in `why` what needs it: the boot says it
    /// back, in its log, to whoever reads what the app allows.
    pub fn ask(mut self, directive: Directive, source: &'static str, why: &'static str) -> Self {
        self.asks.push((directive, source, why));
        self
    }

    /// Asks for WebAssembly, and says in `why` what needs it. The app's one switch for it
    /// (`ui.policy.wasm`) opens it for every script on every page.
    pub fn wasm(mut self, why: &'static str) -> Self {
        self.wasm = Some(why);
        self
    }

    /// What was asked for: the directive, the source and why, in the order asked.
    pub fn asks(&self) -> impl Iterator<Item = (Directive, &'static str, &'static str)> + '_ {
        self.asks.iter().copied()
    }
}

/// The content security policy of the app's pages, composed once at boot from the toolkit's own
/// and what the app's `ui.policy` section allows. Ask the app's context for it (the example
/// below) to learn what the pages may load: a plugin's component that needs a source it is not
/// allowed shows its fallback instead of a picture the browser would block.
///
/// There is no way to build one: the boot does, and it is the one place the policy is widened.
///
/// ```no_run
/// use navree_kernel::AppContext;
/// use navree_ui::{Directive, PagePolicy};
///
/// async fn tiles_allowed(ctx: AppContext) -> bool {
///     let policy = ctx.get::<PagePolicy>().await.expect("the toolkit's page policy");
///     policy.allows(Directive::ImgSrc, "https://tile.openstreetmap.org/15/23500/13700.png")
/// }
/// ```
#[derive(Clone, Debug)]
pub struct PagePolicy {
    header: String,
    /// `header` as the response carries it, made once.
    value: HeaderValue,
    wasm: bool,
    /// The sources the app allowed, by directive: every key's, each once.
    allowed: BTreeMap<Directive, Vec<String>>,
}

impl PagePolicy {
    /// The policy header's value: the toolkit's, with what the app allowed.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// Whether the pages may load `source` for `directive`, as the header says it: the toolkit's
    /// own sources (`'self'` for every directive but media, and `data:` for pictures) and the
    /// allowed ones. A keyword is matched as it is written, `data:` and `blob:` by scheme, and an
    /// origin by scheme, host and port, a leading `*.` standing for one or more labels; a path or
    /// a query in `source` is left out of the match.
    pub fn allows(&self, directive: Directive, source: &str) -> bool {
        let own: &[&str] = match directive {
            Directive::ImgSrc => &["'self'", "data:"],
            Directive::FontSrc | Directive::ConnectSrc => &["'self'"],
            Directive::MediaSrc => &[],
        };
        let allowed = self
            .allowed
            .get(&directive)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let asked = source.to_ascii_lowercase();
        own.iter()
            .copied()
            .chain(allowed.iter().map(String::as_str))
            .any(|listed| covers(listed, &asked))
    }

    /// Whether the app allowed WebAssembly on its pages (`ui.policy.wasm`).
    pub fn allows_wasm(&self) -> bool {
        self.wasm
    }

    /// Whether this is the toolkit's own policy: the app allowed nothing.
    pub fn is_the_toolkits(&self) -> bool {
        self.header == PAGE_CSP
    }
}

/// Whether `listed`, a source of the policy, covers `asked`, a source written in lower case.
fn covers(listed: &str, asked: &str) -> bool {
    if listed == "'self'" {
        return asked == listed;
    }
    if listed == "data:" || listed == "blob:" {
        return asked.starts_with(listed);
    }
    let (Some((scheme, listed)), Some((asked_scheme, after_colon))) =
        (listed.split_once("://"), asked.split_once(':'))
    else {
        return false;
    };
    if scheme != asked_scheme {
        return false;
    }
    let mut slashes = after_colon
        .chars()
        .take_while(|ch| matches!(ch, '/' | '\\'));
    if slashes.next().is_none() || slashes.next().is_none() {
        return false;
    }
    let rest = after_colon.trim_start_matches(['/', '\\']);
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or(rest);
    if authority.contains('@') {
        return false;
    }
    let (Some((host, port)), Some((listed_host, listed_port))) =
        (host_and_port(authority), host_and_port(listed))
    else {
        return false;
    };
    let default = match scheme {
        "https" | "wss" => Some(443),
        "http" | "ws" => Some(80),
        _ => None,
    };
    port.or(default) == listed_port.or(default) && host_covered(listed_host, host)
}

/// Whether the host `listed` (which may start with `*.`) names `host`: a name is its own, and a
/// wildcard stands for one label or more in front of the name after it.
fn host_covered(listed: &str, host: &str) -> bool {
    match listed.strip_prefix("*.") {
        Some(name) => host.strip_suffix(name).is_some_and(|front| {
            front
                .strip_suffix('.')
                .is_some_and(|labels| labels.split('.').all(|label| !label.is_empty()))
        }),
        None => listed == host,
    }
}

/// Why `source` is no source of `directive`: what the directive takes, as the boot says it.
/// `None` when it is one. In production and staging (`production`), `http://localhost` and
/// `http://127.0.0.1` are not sources.
pub(crate) fn source_problem(
    directive: Directive,
    source: &str,
    production: bool,
) -> Option<&'static str> {
    let taken = match source {
        "'self'" => true,
        "data:" => directive != Directive::ConnectSrc,
        "blob:" => matches!(directive, Directive::ImgSrc | Directive::MediaSrc),
        _ => match source.split_once("://") {
            Some(("https", origin)) => is_origin(origin),
            Some(("wss", origin)) => directive == Directive::ConnectSrc && is_origin(origin),
            Some(("http", origin)) => !production && is_local(origin),
            _ => false,
        },
    };
    if taken { None } else { Some(directive.forms()) }
}

/// Whether `authority` is a host name in lower case with at least two labels (not an address in
/// numbers), which may start with `*.` and have a port after it.
fn is_origin(authority: &str) -> bool {
    let Some((host, _)) = host_and_port(authority) else {
        return false;
    };
    let name = host.strip_prefix("*.").unwrap_or(host);
    let top = name.rsplit('.').next().unwrap_or_default();
    name.len() <= 253
        && name.split('.').count() >= 2
        && name.split('.').all(is_label)
        && !top.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `authority` is this machine, with a port or without: `localhost` or `127.0.0.1`.
fn is_local(authority: &str) -> bool {
    host_and_port(authority).is_some_and(|(host, _)| host == "localhost" || host == "127.0.0.1")
}

/// Whether `source` is `http://localhost` or `http://127.0.0.1`, with a port or without.
fn is_local_source(source: &str) -> bool {
    source.strip_prefix("http://").is_some_and(is_local)
}

/// Whether `source` is an https origin whose host starts with a wildcard.
fn is_wildcard(source: &str) -> bool {
    source
        .split_once("://")
        .is_some_and(|(_, origin)| origin.starts_with("*."))
}

/// A DNS label in lower case: letters, digits and dashes, not starting or ending with a dash,
/// at most 63 characters.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The host and the port of `authority` (`host` or `host:port`); `None` when what stands after
/// the colon is not a port.
fn host_and_port(authority: &str) -> Option<(&str, Option<u16>)> {
    match authority.split_once(':') {
        None => Some((authority, None)),
        Some((host, port)) => Some((host, Some(port_of(port)?))),
    }
}

/// A port as an origin writes it: digits only, no leading zero, from 1 to 65535.
fn port_of(text: &str) -> Option<u16> {
    let written = !text.is_empty()
        && text.len() <= 5
        && !text.starts_with('0')
        && text.bytes().all(|byte| byte.is_ascii_digit());
    if written { text.parse().ok() } else { None }
}

/// What one module asked for, and for whom.
struct Ask {
    /// The namespace of the module's plugin, or `app`.
    namespace: String,
    directive: Directive,
    source: &'static str,
    why: &'static str,
}

/// Builds the app's policy: reads the `ui.policy` section and what the modules asked for, checks
/// both, says in the log what was asked for and what the app allowed, and composes the header.
/// Fails with every problem, one on each line, when a key or a source is outside the rules.
pub(crate) async fn build(ctx: &AppContext) -> Result<PagePolicy, BoxError> {
    let mut errors: Vec<String> = Vec::new();
    let config = ctx.try_get::<Config>().await;
    let production = config
        .as_deref()
        .is_some_and(Config::behaves_like_production);
    let options = match config.as_deref() {
        Some(config) => {
            unknown_keys(config, &mut errors);
            match config.section_or_default::<PagePolicyOptions>(PagePolicyOptions::KEY) {
                Ok(options) => options,
                Err(error) => {
                    errors.push(error.to_string());
                    PagePolicyOptions::default()
                }
            }
        }
        None => PagePolicyOptions::default(),
    };
    if let Err(problems) = options.check() {
        errors.extend(problems.lines().map(str::to_owned));
    }
    if production {
        for (key, sources) in &options.allow {
            for directive in Directive::ALL.iter().copied() {
                for (at, source) in sources.of(directive).iter().enumerate() {
                    if is_local_source(source) {
                        errors.push(format!(
                            "ui.policy.allow.{key}.{}[{at}] points at this machine (localhost), which is refused in production and staging",
                            directive.key()
                        ));
                    }
                }
            }
        }
    }

    // What the modules ask for: under the namespace of the plugin each module acts for, or `app`
    // for a module that acts for none.
    let plugin_reports = ctx.plugins().plugins;
    let mut asks: Vec<Ask> = Vec::new();
    let mut wasm_asks: Vec<(String, &'static str)> = Vec::new();
    for entry in ctx.contributions::<PageSources>() {
        let plugin = crate::extension::plugin_of(&plugin_reports, entry.module.id)
            .map(|report| &report.info);
        let namespace = plugin.map_or(APP, |info| info.namespace).to_owned();
        let who = match plugin {
            Some(info) => format!("plugin `{}`", info.name),
            None => format!("module `{}`", entry.module.name),
        };
        for (directive, source, why) in entry.value.asks() {
            if let Some(forms) = source_problem(directive, source, production) {
                errors.push(format!(
                    "{who} asks for {} `{source}`, which is not a source the page policy takes ({forms})",
                    directive.as_str()
                ));
            }
            asks.push(Ask {
                namespace: namespace.clone(),
                directive,
                source,
                why,
            });
        }
        if let Some(why) = entry.value.wasm {
            wasm_asks.push((namespace, why));
        }
    }
    if !errors.is_empty() {
        errors.dedup();
        return Err(errors.join("\n").into());
    }

    // The allowed sources as the header lists them: the keys in name order with `app` first, the
    // sources in the order written, each once.
    let mut keys: Vec<&str> = options.allow.keys().map(String::as_str).collect();
    keys.sort_by(|one, other| (*one != APP).cmp(&(*other != APP)).then(one.cmp(other)));
    let mut allowed: BTreeMap<Directive, Vec<String>> = BTreeMap::new();
    for key in keys {
        for directive in Directive::ALL.iter().copied() {
            let listed = allowed.entry(directive).or_default();
            for source in options.allow[key].of(directive) {
                if !listed.contains(source) {
                    listed.push(source.clone());
                }
            }
        }
    }
    allowed.retain(|_, listed| !listed.is_empty());
    let header = composed(&allowed, options.wasm);
    let value = HeaderValue::from_str(&header)?;

    let plugins: Vec<&'static str> = plugin_reports
        .iter()
        .map(|plugin| plugin.info.namespace)
        .collect();
    tell(&options, &asks, &wasm_asks, &plugins, &header);
    Ok(PagePolicy {
        header,
        value,
        wasm: options.wasm,
        allowed,
    })
}

/// The keys under a namespace of `ui.policy.allow` that are not one of the four directives: a
/// key for a script, a style, a worker or a frame is not ignored, it stops the boot.
fn unknown_keys(config: &Config, errors: &mut Vec<String>) {
    let Ok(raw) = config.section_or_default::<serde_json::Value>(PagePolicyOptions::KEY) else {
        return;
    };
    let Some(allow) = raw.get("allow").and_then(serde_json::Value::as_object) else {
        return;
    };
    let known = Directive::ALL
        .iter()
        .map(|directive| directive.key())
        .collect::<Vec<_>>()
        .join(", ");
    for (key, table) in allow {
        let Some(table) = table.as_object() else {
            continue;
        };
        for name in table.keys() {
            if !Directive::ALL
                .iter()
                .any(|directive| directive.key() == name)
            {
                errors.push(format!(
                    "ui.policy.allow.{key}: unknown key `{name}` (known: {known}); a page's scripts, styles, workers and frames come from this app only"
                ));
            }
        }
    }
}

/// The policy header: the toolkit's directives in their order, each allowed source after the
/// toolkit's own, `media-src` after `connect-src` when it has sources, and `'wasm-unsafe-eval'`
/// after the scripts' own source when WebAssembly is on. With nothing to add, the toolkit's own
/// header, exactly.
fn composed(allowed: &BTreeMap<Directive, Vec<String>>, wasm: bool) -> String {
    if allowed.is_empty() && !wasm {
        return PAGE_CSP.to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    for part in PAGE_CSP.split("; ") {
        let name = part.split(' ').next().unwrap_or(part);
        let mut line = part.to_owned();
        let directive = match name {
            "script-src" => {
                if wasm {
                    line.push_str(" 'wasm-unsafe-eval'");
                }
                None
            }
            "img-src" => Some(Directive::ImgSrc),
            "connect-src" => Some(Directive::ConnectSrc),
            "font-src" => Some(Directive::FontSrc),
            _ => None,
        };
        if let Some(listed) = directive.and_then(|directive| allowed.get(&directive)) {
            for source in listed {
                if !line.split(' ').skip(1).any(|have| have == source) {
                    line.push(' ');
                    line.push_str(source);
                }
            }
        }
        parts.push(line);
        if name == "connect-src"
            && let Some(listed) = allowed.get(&Directive::MediaSrc)
        {
            parts.push(format!("media-src {}", listed.join(" ")));
        }
    }
    parts.join("; ")
}

/// Says in the log what the modules asked for and what the app allowed: a warning for what was
/// asked and not allowed, for what is allowed and not asked, for a key no plugin has and for a
/// wildcard; an info line for what was asked and allowed and for the header the pages carry.
fn tell(
    options: &PagePolicyOptions,
    asks: &[Ask],
    wasm_asks: &[(String, &'static str)],
    plugins: &[&'static str],
    header: &str,
) {
    for ask in asks {
        let Ask {
            namespace,
            directive,
            source,
            why,
        } = ask;
        let allowed = options
            .allow
            .get(namespace)
            .is_some_and(|sources| sources.of(*directive).iter().any(|listed| listed == source));
        if allowed {
            tracing::info!(
                target: "navree::ui",
                "page policy: plugin `{namespace}` asks for {} {source} ({why}): allowed",
                directive.as_str()
            );
        } else {
            tracing::warn!(
                target: "navree::ui",
                "page policy: plugin `{namespace}` asks for {} {source} ({why}), which the app does not allow; what needs it shows its fallback (allow it with ui.policy.allow.{namespace}.{})",
                directive.as_str(),
                directive.key()
            );
        }
    }
    if !options.wasm {
        for (namespace, why) in wasm_asks {
            tracing::warn!(
                target: "navree::ui",
                "page policy: plugin `{namespace}` asks for WebAssembly ({why}), which the app does not allow (allow it with ui.policy.wasm = true, which opens it for every script)"
            );
        }
    } else {
        let mut asked_by: Vec<&str> = wasm_asks
            .iter()
            .map(|(namespace, _)| namespace.as_str())
            .collect();
        asked_by.sort_unstable();
        asked_by.dedup();
        let asked_by = if asked_by.is_empty() {
            "none".to_owned()
        } else {
            asked_by.join(", ")
        };
        tracing::info!(
            target: "navree::ui",
            "page policy: WebAssembly is allowed for every script on every page (ui.policy.wasm); asked for by: {asked_by}"
        );
    }
    for (key, sources) in &options.allow {
        let named_by_a_plugin = plugins.contains(&key.as_str());
        if key != APP && !named_by_a_plugin {
            tracing::warn!(
                target: "navree::ui",
                "page policy: ui.policy.allow.{key} names no plugin of this app; its sources are allowed on every page all the same"
            );
        }
        for directive in Directive::ALL.iter().copied() {
            for source in sources.of(directive) {
                if is_wildcard(source) {
                    tracing::warn!(
                        target: "navree::ui",
                        "page policy: ui.policy.allow.{key}.{} allows the wildcard {source}: anyone who can hold a name under it can receive what a page sends there",
                        directive.key()
                    );
                }
                let asked = asks.iter().any(|ask| {
                    ask.namespace == *key && ask.directive == directive && ask.source == source
                });
                if named_by_a_plugin && !asked {
                    tracing::warn!(
                        target: "navree::ui",
                        "page policy: ui.policy.allow.{key} allows {} {source}, which plugin `{key}` does not ask for",
                        directive.as_str()
                    );
                }
            }
        }
    }
    if header != PAGE_CSP {
        tracing::info!(
            target: "navree::ui",
            "page policy: every page but the fresh ones is served with: {header}"
        );
    }
}

/// Puts the app's policy on `response` when the toolkit's own is on it: a header that is exactly
/// [`PAGE_CSP`] is replaced, and any other policy, or none, is left as it is. A
/// page that starts clean keeps the toolkit's policy. Nothing is done when the app allowed
/// nothing.
pub(crate) fn apply(policy: &PagePolicy, response: &mut Response) {
    if policy.is_the_toolkits() || is_fresh_answer(response) {
        return;
    }
    let headers = response.headers_mut();
    let toolkits = {
        let mut values = headers.get_all(CONTENT_SECURITY_POLICY).iter();
        matches!(
            (values.next(), values.next()),
            (Some(only), None) if only.as_bytes() == PAGE_CSP.as_bytes()
        )
    };
    if toolkits {
        headers.insert(CONTENT_SECURITY_POLICY, policy.value.clone());
    }
}

/// The header on the answer of a page that starts clean: the runtime refuses to draw such an
/// answer into a page that does not, whatever it holds.
pub(crate) const FRESH_HEADER: HeaderName = HeaderName::from_static("x-navree-fresh");

/// On the response of a page that starts clean, so the layer that composes the app's policy keeps
/// the toolkit's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FreshAnswer;

/// Says on `response` that it is the answer of a page that starts clean: the header the runtime
/// reads, and the mark the layer that composes the app's policy reads.
pub(crate) fn mark_fresh(response: &mut Response) {
    response
        .headers_mut()
        .insert(FRESH_HEADER, HeaderValue::from_static("1"));
    response.extensions_mut().insert(FreshAnswer);
}

/// Whether `response` is the answer of a page that starts clean: it carries the mark, or the
/// header the mark's page writes.
fn is_fresh_answer(response: &Response) -> bool {
    response.extensions().get::<FreshAnswer>().is_some()
        || response.headers().contains_key(FRESH_HEADER)
}

/// The most routes each of the two password warnings names, so the log stays short.
const MOST_ROUTES: usize = 256;

/// The deepest a walk of a tree goes, as the renderer's: what is below is left out.
const MAX_DEPTH: u32 = 512;

tokio::task_local! {
    /// What the handlers of the request in hand say of the password fields they send.
    static FRESH: Arc<Scope>;
    /// Marks an internal `GET` probe that classifies whether a route's page is fresh.
    static PROBING: ();
}

/// Runs `future` inside a fresh-page request scope, for paths such as `ErrorPages::render` that
/// execute outside [`fresh_layer`].
pub(crate) async fn in_fresh_scope<F: std::future::Future>(future: F) -> F::Output {
    let scope = Arc::new(Scope::default());
    FRESH.scope(scope, future).await
}

/// Remembers which route patterns answer with a fresh page in an app where pages that start clean
/// are on, and probes `GET` once when an error page or refused form runs at an address whose
/// `GET` handler has not run yet.
#[derive(Default)]
pub(crate) struct FreshRoutes {
    fresh: Mutex<BTreeSet<String>>,
    plain: Mutex<BTreeSet<String>>,
    router: tokio::sync::OnceCell<navree_http::AppRouter>,
}

impl FreshRoutes {
    /// Records whether `response` answered a fresh page or a plain page on its route pattern.
    pub(crate) fn note_response(&self, response: &Response) {
        let Some(route) = response.extensions().get::<Arc<RouteMeta>>() else {
            return;
        };
        let path = route.full_path.as_str();
        if is_fresh_answer(response) {
            self.plain
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(path);
            self.fresh
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(path.to_owned());
        } else if response.status().is_success()
            && response
                .extensions()
                .get::<crate::view::ViewAnswer>()
                .is_some()
        {
            let already_fresh = self
                .fresh
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(path);
            if !already_fresh {
                self.plain
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(path.to_owned());
            }
        }
    }

    /// Synchronous lookup against routes already classified.
    pub(crate) fn is_known_fresh(&self, route_path: Option<&str>, uri_path: &str) -> Option<bool> {
        let fresh = self.fresh.lock().unwrap_or_else(PoisonError::into_inner);
        let plain = self.plain.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(route_path) = route_path {
            if fresh.contains(route_path) {
                return Some(true);
            }
            if plain.contains(route_path) {
                return Some(false);
            }
        }
        if fresh.contains(uri_path) {
            return Some(true);
        }
        if plain.contains(uri_path) {
            return Some(false);
        }
        None
    }

    async fn probe(&self, ctx: &AppContext, request: Request) -> bool {
        use std::time::Duration;
        use tower::ServiceExt;

        if PROBING.try_with(|_| ()).is_ok() {
            return false;
        }
        let Ok(router) = self
            .router
            .get_or_try_init(|| navree_http::build_router(ctx))
            .await
        else {
            return false;
        };
        let router = router.clone();
        let attempt = PROBING.scope((), router.oneshot(request));
        match tokio::time::timeout(Duration::from_millis(500), attempt).await {
            Ok(Ok(response)) => {
                self.note_response(&response);
                is_fresh_answer(&response)
            }
            _ => false,
        }
    }

    /// Whether the address of `parts` belongs to a fresh page, probing `GET` once if needed.
    pub(crate) async fn check_parts(&self, ctx: &AppContext, parts: &http::request::Parts) -> bool {
        let route_path = parts
            .extensions
            .get::<Arc<RouteMeta>>()
            .map(|route| route.full_path.as_str());
        if let Some(known) = self.is_known_fresh(route_path, parts.uri.path()) {
            return known;
        }
        let Ok(uri) = http::Uri::try_from(parts.uri.path()) else {
            return false;
        };
        let mut probe_parts = parts.clone();
        probe_parts.method = http::Method::GET;
        probe_parts.uri = uri;
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml"),
        );
        for kept in [
            http::header::COOKIE,
            http::header::HOST,
            http::header::USER_AGENT,
        ] {
            for value in parts.headers.get_all(&kept) {
                headers.append(kept.clone(), value.clone());
            }
        }
        probe_parts.headers = headers;
        let request = Request::from_parts(probe_parts, navree_http::body::Body::empty());
        self.probe(ctx, request).await
    }

    /// Whether the address of `error_ctx` belongs to a fresh page, probing `GET` once if needed.
    pub(crate) async fn check_error(&self, error_ctx: &navree_http::ErrorContext) -> bool {
        let Some(route) = error_ctx.route.as_ref() else {
            return false;
        };
        if let Some(known) =
            self.is_known_fresh(Some(route.full_path.as_str()), error_ctx.uri.path())
        {
            return known;
        }
        let Ok(mut builder) = http::Request::builder()
            .method(http::Method::GET)
            .uri(error_ctx.uri.path())
            .header(
                http::header::ACCEPT,
                HeaderValue::from_static("text/html,application/xhtml+xml"),
            )
            .body(navree_http::body::Body::empty())
        else {
            return false;
        };
        for kept in [
            http::header::COOKIE,
            http::header::HOST,
            http::header::USER_AGENT,
        ] {
            for value in error_ctx.headers.get_all(&kept) {
                builder.headers_mut().append(kept.clone(), value.clone());
            }
        }
        let peer =
            std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 49152);
        builder
            .extensions_mut()
            .insert(navree_http::serve::ConnectInfo(peer));
        self.probe(&error_ctx.ctx, builder).await
    }
}

/// What the handlers of one request noted: a page that holds a password field and is not fresh,
/// and operations that carry one.
#[derive(Default)]
struct Scope {
    page_password: AtomicBool,
    ops_password: AtomicBool,
}

/// Whether the current task runs the handler of a request in an app where pages that start clean
/// are on. A page is marked fresh, and a password field is noted, only there: a page written
/// outside a request, or in an app of today, writes and notes nothing.
pub(crate) fn in_scope() -> bool {
    FRESH.try_with(|_| ()).is_ok()
}

/// Notes that the page being answered holds a password field and is not fresh.
pub(crate) fn note_page_password() {
    let _ = FRESH.try_with(|scope| scope.page_password.store(true, Ordering::Relaxed));
}

/// Notes that the operations being answered carry a password field.
pub(crate) fn note_ops_password() {
    let _ = FRESH.try_with(|scope| scope.ops_password.store(true, Ordering::Relaxed));
}

/// Whether `node` holds an input that takes a password (type `password`) or whose `autocomplete`
/// names a password or a one-time code (`current-password`, `new-password` or `one-time-code`).
/// Only elements are read: a field inside markup written as a string is not seen.
pub(crate) fn holds_password(node: &Node) -> bool {
    holds(node, 0)
}

/// One step of [`holds_password`].
fn holds(node: &Node, depth: u32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match node {
        Node::Element(element) => {
            takes_password(element) || element.children.iter().any(|child| holds(child, depth + 1))
        }
        Node::Fragment(nodes) => nodes.iter().any(|child| holds(child, depth + 1)),
        _ => false,
    }
}

/// Whether `element` is an input for a password or a code that signs a person in.
fn takes_password(element: &Element) -> bool {
    if !element.tag.eq_ignore_ascii_case("input") {
        return false;
    }
    let text = |name: &str| element.get(name).and_then(AttrValue::as_text);
    text("type").is_some_and(|kind| kind.eq_ignore_ascii_case("password"))
        || text("autocomplete").is_some_and(|hints| {
            hints.split_ascii_whitespace().any(|hint| {
                ["current-password", "new-password", "one-time-code"]
                    .iter()
                    .any(|credential| hint.eq_ignore_ascii_case(credential))
            })
        })
}

/// The routes the log has named, for each of the two password warnings, at most 256 of each.
#[derive(Default)]
pub(crate) struct Warned {
    pages: Mutex<BTreeSet<String>>,
    ops: Mutex<BTreeSet<String>>,
}

impl Warned {
    /// Whether `path` is the first to be named in `routes`, which then keeps it: never once
    /// `routes` holds as many as the log will name.
    fn first(routes: &Mutex<BTreeSet<String>>, path: &str) -> bool {
        let mut routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
        routes.len() < MOST_ROUTES && routes.insert(path.to_owned())
    }
}

/// The layer around the handlers of an app where pages that start clean are on: it makes the
/// request's scope current for them, and when a page that is not fresh held a password field, or
/// a reply sent one as operations, names the route in the log, once. The route is the pattern the
/// request matched, never the address asked for.
pub(crate) async fn fresh_layer(
    warned: Arc<Warned>,
    routes: Arc<FreshRoutes>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(routes.clone());
    let scope = Arc::new(Scope::default());
    let response = FRESH.scope(scope.clone(), next.run(request)).await;
    routes.note_response(&response);
    if PROBING.try_with(|_| ()).is_ok() {
        return response;
    }
    let page = scope.page_password.load(Ordering::Relaxed);
    let ops = scope.ops_password.load(Ordering::Relaxed);
    if (page || ops)
        && let Some(route) = response.extensions().get::<Arc<RouteMeta>>()
    {
        let path = route.full_path.as_str();
        if page && Warned::first(&warned.pages, path) {
            tracing::warn!(
                target: "navree::ui",
                "page {path} holds a password field and is not fresh: call .fresh() on its View or AppPage"
            );
        }
        if ops && Warned::first(&warned.ops, path) {
            tracing::warn!(
                target: "navree::ui",
                "route {path} sends a password field as ops into a page that may hold plugin code: draw it on a fresh page and answer with a redirect to it"
            );
        }
    }
    response
}
