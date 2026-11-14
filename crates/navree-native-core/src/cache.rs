//! The last tree of each address: shown at once while a fresh one is fetched.
//!
//! The core keeps a bounded number of answers in memory, as they came. When
//! the app's build says so, it also hands the client to keep between launches,
//! under keys the client need not understand, the shell, the list of pages
//! kept and their owner (`boot`), and each page whose answer said it may be
//! kept ([`navree_native::KEEP_HEADER`], sent when the page's `Cache-Control`
//! does not say `no-store`) under `page:<address>`. An answer that does not
//! say so removes what was kept for its address.
//!
//! What is kept has an **owner**: the person, or the visitor, its pages were
//! answered for, written in `boot` beside the list. A kept page may be read
//! only when this launch's shell has answered over the network, as a shell,
//! and named that owner ([`Cache::readable`]): both ways a page is read ask
//! it, and every answer of the shell asks it again. A shell that names
//! someone else empties what is kept before anything is read. An answer that
//! is not a shell names nobody and makes nothing readable. A `boot` with no
//! owner, or whose kept shell does not read, is emptied, not taken.
//!
//! A kept tree is only ever a head start: it is marked as kept, nothing is
//! sent from it, and the fresh answer takes its place. A write, a new build,
//! a change of person and a sign-out empty what is kept.

use serde::{Deserialize, Serialize};

use navree_native::node::Method;

use crate::Core;
use crate::effect::Effect;
use crate::event::Response;
use crate::state::{Asked, Content, Pending, ScreenId};

/// A person as the core compares people: a name and an email. `None` is a
/// visitor, signed in as nobody.
pub(crate) type Person = Option<(String, Option<String>)>;

/// Whose the pages the client keeps are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Owner {
    /// What the client keeps has not been read yet in this launch.
    #[default]
    Unread,
    /// Nothing is kept: this launch's shell decides whose pages are kept next.
    Empty,
    /// The pages kept were answered for this person.
    Of(Person),
}

/// The answers kept in memory, and what the client keeps.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    /// The last answer of each address, the newest last: address, status, body.
    pages: Vec<(String, u16, String)>,
    /// The addresses the client keeps a page for, the newest last.
    pub disk: Vec<String>,
    /// The shell as it came, for the client to keep.
    pub shell: Option<String>,
    /// Whose the pages the client keeps are.
    pub owner: Owner,
    /// The person this launch's shell named, when it answered over the
    /// network as a shell. An answer that is not a shell never sets it.
    pub named: Option<Person>,
    /// The pages taken away before what was kept had been read: they stay
    /// away when the list is read.
    removed: Vec<String>,
    /// The build the pages kept were answered by, as `boot` said, until this
    /// launch's own build is known.
    kept_build: Option<String>,
    /// The pages asked of the client since what is kept was last emptied:
    /// an answer for any other is dropped.
    asked: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Boot {
    shell: String,
    #[serde(default)]
    pages: Vec<String>,
    /// Whose the pages are. A `boot` without it was written by an older
    /// build, and is not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<Kept>,
    /// The build the pages were answered by: another build empties them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    build: Option<String>,
}

/// The owner of the pages kept, as `boot` holds it.
#[derive(Serialize, Deserialize)]
struct Kept {
    /// The person, or `null` for a visitor.
    person: Person,
}

#[derive(Serialize, Deserialize)]
struct Page {
    status: u16,
    body: String,
}

impl Cache {
    fn get(&self, url: &str) -> Option<(u16, &str)> {
        self.pages
            .iter()
            .find(|(kept, _, _)| kept == url)
            .map(|(_, status, body)| (*status, body.as_str()))
    }

    fn put(&mut self, url: &str, status: u16, body: String, limit: usize) {
        self.pages.retain(|(kept, _, _)| kept != url);
        self.pages.push((url.to_owned(), status, body));
        if self.pages.len() > limit {
            self.pages.remove(0);
        }
    }

    /// Whether what the client keeps may be read, or written to: this
    /// launch's shell answered over the network as a shell, and the person it
    /// named owns what is kept. The one question every read and write asks.
    pub(crate) fn readable(&self) -> bool {
        matches!((&self.owner, &self.named), (Owner::Of(owner), Some(named)) if owner == named)
    }
}

impl Core {
    /// The key a page is kept under: its whole address as it leaves the core,
    /// so two spellings of one address are one page.
    fn key_of(&self, url: &str) -> String {
        self.inner
            .origin
            .absolute(url)
            .unwrap_or_else(|| url.to_owned())
    }

    /// Reads what an answer says of keeping its page on the device. Only a
    /// whole page, asked for with a `get`, answered with success and with
    /// [`navree_native::KEEP_HEADER`], and whose `cache-control` (when the
    /// client passed it) does not say `no-store`, may be kept; any other answer takes
    /// away what was kept for the address asked, and loses the header. An
    /// answer that never came (no network) changes nothing.
    pub(crate) fn keeping(&mut self, asked: &Asked, mut response: Response) -> Response {
        if response.error.is_some() {
            return response;
        }
        let kept = !asked.delete
            && asked.method == Method::Get
            && asked.region.is_none()
            && (200..300).contains(&response.status)
            && response.header(navree_native::KEEP_HEADER) == Some("1")
            // The answer's own `cache-control`, when the client passed it, is
            // the server's last word.
            && response
                .header("cache-control")
                .is_none_or(|value| !value.to_ascii_lowercase().contains("no-store"));
        if !kept {
            response
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case(navree_native::KEEP_HEADER));
            // A screen's own page is taken away from the device whatever the
            // list in memory says; another request's address only when listed.
            let page = matches!(asked.purpose, Pending::Page { .. });
            self.forget_kept(&asked.url, page);
        }
        response
    }

    /// Takes away what the client keeps for `url`: always when `always`, else
    /// only when the list says it keeps a page there.
    fn forget_kept(&mut self, url: &str, always: bool) {
        if !self.config.cache.persist {
            return;
        }
        let key = self.key_of(url);
        let listed = self.inner.cache.disk.contains(&key);
        if !listed && !always {
            return;
        }
        if self.inner.cache.owner == Owner::Unread && !self.inner.cache.removed.contains(&key) {
            self.inner.cache.removed.push(key.clone());
        }
        self.inner.cache.disk.retain(|kept| *kept != key);
        self.effects.push(Effect::CacheWrite {
            key: format!("page:{key}"),
            value: None,
        });
        if listed {
            self.boot();
        }
    }

    /// Hands the client the shell, the list of pages kept and their owner.
    /// Nothing is written before what was kept has been read and has an
    /// owner: a list written then would stand for nobody's pages.
    fn boot(&mut self) {
        if !self.config.cache.persist {
            return;
        }
        let Owner::Of(person) = self.inner.cache.owner.clone() else {
            return;
        };
        let Some(shell) = self.inner.cache.shell.clone() else {
            return;
        };
        let boot = Boot {
            shell,
            pages: self.inner.cache.disk.clone(),
            owner: Some(Kept { person }),
            build: self.inner.build.clone(),
        };
        self.effects.push(Effect::CacheWrite {
            key: "boot".to_owned(),
            value: serde_json::to_string(&boot).ok(),
        });
    }

    /// Keeps the shell for the next launch.
    pub(crate) fn remember_shell(&mut self, body: String) {
        if self.inner.cache.shell.as_deref() != Some(&body) {
            self.inner.cache.shell = Some(body);
            self.boot();
        }
    }

    /// Keeps the answer a page gave: in memory, and on the device when the
    /// answer said it may be kept (`keep`) and what is kept is this
    /// person's. An answer that did not say so takes away what the device
    /// kept for its address.
    pub(crate) fn remember(&mut self, url: &str, status: u16, body: &str, keep: bool) {
        let limit = self.config.cache.pages;
        if limit == 0 || status >= 400 {
            return;
        }
        self.inner.cache.put(url, status, body.to_owned(), limit);
        if !self.config.cache.persist {
            return;
        }
        if !keep {
            return self.forget_kept(url, false);
        }
        if !self.inner.cache.readable() {
            return;
        }
        let key = self.key_of(url);
        let page = Page {
            status,
            body: body.to_owned(),
        };
        self.effects.push(Effect::CacheWrite {
            key: format!("page:{key}"),
            value: serde_json::to_string(&page).ok(),
        });
        let disk = &mut self.inner.cache.disk;
        let listed = disk.last().is_some_and(|last| *last == key);
        disk.retain(|kept| *kept != key);
        disk.push(key);
        let dropped = (disk.len() > limit).then(|| disk.remove(0));
        if let Some(dropped) = dropped {
            self.effects.push(Effect::CacheWrite {
                key: format!("page:{dropped}"),
                value: None,
            });
        }
        if !listed {
            self.boot();
        }
    }

    /// Empties the pages kept: something was written, or the build changed.
    /// The device's store is emptied whatever the list in memory holds, and
    /// a list read after this is not taken.
    pub(crate) fn forget_pages(&mut self) {
        self.inner.cache.pages.clear();
        self.inner.cache.disk.clear();
        self.inner.cache.asked.clear();
        if !self.config.cache.persist {
            return;
        }
        self.effects.push(Effect::CacheClear);
        if self.inner.cache.owner == Owner::Unread {
            self.inner.cache.owner = Owner::Empty;
            self.owner_settles();
        } else {
            self.boot();
        }
    }

    /// This launch's build is known now: pages kept under another empty.
    /// True when they were emptied.
    pub(crate) fn build_known(&mut self) -> bool {
        let Some(kept) = self.inner.cache.kept_build.take() else {
            return false;
        };
        let other = self
            .inner
            .build
            .as_deref()
            .is_some_and(|build| build != kept);
        if other {
            self.forget_pages();
        }
        other
    }

    /// Empties everything kept: the person changed, or signed out. What the
    /// client keeps is known to be nothing from here on, and a `boot` read
    /// before this is not used.
    pub(crate) fn forget_all(&mut self) {
        self.inner.cache = Cache {
            owner: Owner::Empty,
            ..Cache::default()
        };
        if self.config.cache.persist {
            self.effects.push(Effect::CacheClear);
        }
    }

    /// Empties what the client keeps because it is not this launch's to read.
    fn disown(&mut self) {
        self.inner.cache.pages.clear();
        self.inner.cache.disk.clear();
        self.inner.cache.asked.clear();
        self.inner.cache.owner = Owner::Empty;
        if self.config.cache.persist {
            self.effects.push(Effect::CacheClear);
        }
    }

    /// Gives a new screen the kept tree of its address, when there is one, or
    /// asks the client for it.
    pub(crate) fn head_start(&mut self, id: ScreenId, url: &str) {
        if let Some((status, body)) = self.inner.cache.get(url) {
            let body = body.to_owned();
            self.show_kept(id, status, &body);
        } else {
            self.read_kept(url);
        }
    }

    /// Asks the client for the page it kept for `url`, when it keeps one and
    /// it may be read.
    fn read_kept(&mut self, url: &str) {
        let key = self.key_of(url);
        if self.config.cache.persist
            && self.inner.cache.readable()
            && self.inner.cache.disk.contains(&key)
        {
            if !self.inner.cache.asked.contains(&key) {
                self.inner.cache.asked.push(key.clone());
            }
            self.effects.push(Effect::CacheRead {
                key: format!("page:{key}"),
            });
        }
    }

    /// This launch's shell answered over the network, as a shell, and named
    /// `person`. Asked at every such answer: what is kept for someone else is
    /// emptied before anything is read, and nothing kept becomes this
    /// person's; then the screens still waiting for their first tree ask for
    /// theirs, when they may.
    pub(crate) fn shell_named(&mut self, person: Person) {
        self.inner.cache.named = Some(person.clone());
        self.owner_settles();
    }

    /// Settles whose what is kept is, once both what was kept and this
    /// launch's shell are known, and asks for the pages that may now be read.
    fn owner_settles(&mut self) {
        let Some(named) = self.inner.cache.named.clone() else {
            return;
        };
        match &self.inner.cache.owner {
            Owner::Unread => return,
            Owner::Of(owner) if *owner == named => {}
            Owner::Of(_) => {
                self.disown();
                self.inner.cache.owner = Owner::Of(named);
                self.boot();
            }
            Owner::Empty => {
                self.inner.cache.owner = Owner::Of(named);
                self.boot();
            }
        }
        let waiting: Vec<String> = self
            .view
            .screens
            .values()
            .filter(|screen| screen.loading && matches!(screen.content, Content::Blank))
            .map(|screen| screen.url.clone())
            .collect();
        for url in waiting {
            self.read_kept(&url);
        }
    }

    fn show_kept(&mut self, id: ScreenId, status: u16, body: &str) {
        let Some(from) = self.view.screens.get(&id).map(|view| view.url.clone()) else {
            return;
        };
        let sources = crate::decode::Sources {
            origin: &self.inner.origin,
            from: &from,
        };
        let Ok(read) = crate::decode::envelope_with(body, &self.config, Some(&sources)) else {
            return;
        };
        let (Some(screen), true) = (read.envelope.screen, read.missing.is_empty()) else {
            return;
        };
        let Some(view) = self.view.screens.get(&id) else {
            return;
        };
        if !matches!(view.content, Content::Blank) {
            return;
        }
        let url = view.url.clone();
        self.show_native(id, &url, screen, status, true);
    }

    /// What was kept, `boot`, has been read: taken when it has an owner and
    /// its kept shell reads, emptied otherwise. Read once a launch: what
    /// comes after the first answer, or after everything was emptied, is not
    /// used.
    fn boot_loaded(&mut self, value: Option<String>) {
        if self.inner.cache.owner != Owner::Unread {
            return;
        }
        let Some(value) = value else {
            self.inner.cache.owner = Owner::Empty;
            return self.owner_settles();
        };
        let boot = serde_json::from_str::<Boot>(&value)
            .ok()
            .and_then(|boot| Some((boot.owner?.person, boot.pages, boot.shell, boot.build)));
        let Some((owner, mut pages, shell, build)) = boot else {
            // Not readable, or written by an older build with no owner.
            self.disown();
            return self.owner_settles();
        };
        if !self.inner.has_shell && !self.shell_from_cache(&shell) {
            // A kept shell that does not read says nothing of whose the pages are.
            self.disown();
            return self.owner_settles();
        }
        // What was taken away before the list was read stays away.
        let removed = std::mem::take(&mut self.inner.cache.removed);
        let before = pages.len();
        pages.retain(|kept| !removed.contains(kept));
        self.inner.cache.disk = pages;
        self.inner.cache.owner = Owner::Of(owner);
        self.inner.cache.kept_build = build;
        // This launch's build may be known before the list was read.
        let emptied = self.inner.build.is_some() && self.build_known();
        if !emptied && self.inner.cache.disk.len() != before {
            self.boot();
        }
        self.owner_settles();
    }

    /// What the client kept under `key` has been read.
    pub(crate) fn cache_loaded(&mut self, key: &str, value: Option<String>) {
        if key == "boot" {
            return self.boot_loaded(value);
        }
        let Some(value) = value else {
            return;
        };
        if let Some(kept) = key.strip_prefix("page:") {
            // A read asked before what is kept was last emptied, or never
            // asked, is dropped: it may be someone else's page.
            let Some(at) = self
                .inner
                .cache
                .asked
                .iter()
                .position(|asked| asked == kept)
            else {
                return;
            };
            self.inner.cache.asked.remove(at);
            if !self.inner.cache.readable() {
                return;
            }
            let Ok(page) = serde_json::from_str::<Page>(&value) else {
                return;
            };
            // Only a screen that still shows nothing takes it: the fresh answer may have won.
            let waiting: Vec<ScreenId> = self
                .view
                .screens
                .values()
                .filter(|screen| {
                    screen.loading
                        && matches!(screen.content, Content::Blank)
                        && self.key_of(&screen.url) == kept
                })
                .map(|screen| screen.id)
                .collect();
            for id in waiting {
                self.show_kept(id, page.status, &page.body);
            }
        }
    }
}
