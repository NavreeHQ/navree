//! What a client stores so the app starts again where the person was, after
//! the system ended it to make room.
//!
//! A snapshot holds where the person was and what only they knew: the stacks
//! of each destination and what was presented over them, the address and the
//! title of every screen, the part each was opened from, what they had typed
//! into forms and not sent, and which sheets were open. It holds nothing the
//! server gives again: no tree,
//! no page, no message. And nothing secret: no cookie, no token of a form or
//! of a push service, no ticket, no verifier of a sign-in, nothing typed into
//! a password field, and no file.
//!
//! Starting from one (`restore`, in place of `start`) fetches the shell as
//! any start does. Only when the shell names the same person as the snapshot
//! was taken for are the stacks put back, each screen with its address and
//! title and nothing else; the one in front is fetched at once and the others
//! as they come to the front. What was typed goes back into a screen when its
//! page arrives, into the fields that are still there and still of the same
//! kind. A snapshot of another version, another origin or another person is
//! not used, and the app starts as it does the first time.

use std::collections::BTreeMap;

use navree_native::Input;
use navree_native::action::Present;
use navree_native::input::TextKind;
use navree_native::node::Node;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Core;
use crate::effect::Change;
use crate::event::FieldValue;
use crate::state::{Modal, Query, Restored, ScreenId, ScreenView, ViewState};
use crate::tree::{fits, roots, value_of, walk};

/// The version of the snapshot's form. One of another version is not used.
pub const SNAPSHOT_VERSION: u32 = 1;

/// One screen of a snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptScreen {
    /// Its address.
    pub url: String,
    /// Its title, to show while its page is on its way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the person had typed and not sent, by the field's id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldValue>,
    /// The sheets and dialogs of the page that were open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open: Vec<String>,
    /// The id of the part whose action opened the screen, on the screen
    /// under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// The stack of one destination, named by the destination's address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptStack {
    /// The destination's address, as the shell names it.
    pub href: String,
    /// Its screens, the first at the bottom.
    pub stack: Vec<KeptScreen>,
}

/// A stack presented over the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptModal {
    /// How it was presented.
    pub present: Present,
    /// Its screens.
    pub stack: Vec<KeptScreen>,
}

/// Where the person was, to start again from. See the module's words for
/// what it holds and what it never does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The version of this form.
    pub version: u32,
    /// The app's origin.
    pub origin: String,
    /// A fingerprint of who was signed in: a hash, never a name. None when
    /// nobody was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<String>,
    /// The address of the destination that was selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The stacks that held more than nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destinations: Vec<KeptStack>,
    /// What was presented over them, the last on top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modals: Vec<KeptModal>,
}

/// A name for the person a shell is for, which says nothing of them.
fn person_of(user: Option<&crate::cache::Who>) -> Option<String> {
    user.map(|who| match who {
        crate::cache::Who::Named(name, email) => crate::sign_in::fingerprint(&format!(
            "navree-person\n{name}\n{}",
            email.as_deref().unwrap_or("")
        )),
        crate::cache::Who::Id { id } => {
            crate::sign_in::fingerprint(&format!("navree-person-id\n{id}"))
        }
    })
}

/// Whether what a field holds may be kept: never a password, never a file.
fn may_keep(input: &Input, value: &FieldValue) -> bool {
    let secret = matches!(input, Input::Text(text) if text.kind == TextKind::Password);
    !secret && !matches!(input, Input::File(_)) && !matches!(value, FieldValue::Files { .. })
}

/// What a screen holds that only the person knew.
fn kept(view: &ScreenView) -> KeptScreen {
    let mut fields = BTreeMap::new();
    if let Some(screen) = view.screen() {
        let held: BTreeMap<&String, &FieldValue> = view
            .forms
            .values()
            .flat_map(|form| form.fields.iter().map(|(id, field)| (id, &field.value)))
            .collect();
        for root in roots(screen) {
            walk(root, &mut |node| {
                if let Node::Field(field) = node
                    && let Some(id) = &field.id
                    && let Some(value) = held.get(id)
                    // What the page itself came with is fetched again with it.
                    && **value != value_of(&field.input)
                    && may_keep(&field.input, value)
                {
                    fields.insert(id.clone(), (*value).clone());
                }
            });
        }
    }
    KeptScreen {
        url: view.url.clone(),
        title: view.title.clone(),
        fields,
        open: view.overlays.clone(),
        from: view.from.clone(),
    }
}

impl Core {
    /// One part of the view state as JSON, or, for the snapshot, what the
    /// core gives a client to store.
    pub fn part(&self, query: &Query) -> Value {
        match query {
            Query::Snapshot => serde_json::to_value(self.snapshot()).unwrap_or(Value::Null),
            other => self.view.part(other),
        }
    }

    /// Where the person is now, for a client to store. See [`Snapshot`].
    pub fn snapshot(&self) -> Snapshot {
        let view: &ViewState = &self.view;
        let screens = |stack: &[ScreenId]| -> Vec<KeptScreen> {
            stack
                .iter()
                .filter_map(|id| view.screens.get(id))
                .map(kept)
                .collect()
        };
        Snapshot {
            version: SNAPSHOT_VERSION,
            origin: self.config.origin.clone(),
            person: person_of(self.inner.user.as_ref()),
            selected: view
                .destinations
                .get(view.selected)
                .map(|destination| destination.href.clone()),
            destinations: view
                .destinations
                .iter()
                .filter(|destination| !destination.stack.is_empty())
                .map(|destination| KeptStack {
                    href: destination.href.clone(),
                    stack: screens(&destination.stack),
                })
                .collect(),
            // A way in that a session's end put up is not the person's place:
            // the server puts it up again if it is still needed.
            modals: view
                .modals
                .iter()
                .filter(|modal| !modal.locked)
                .map(|modal| KeptModal {
                    present: modal.present,
                    stack: screens(&modal.stack),
                })
                .collect(),
        }
    }

    /// Starts the app from a snapshot: as `start` does, with the snapshot
    /// held until the shell says who is signed in.
    pub(crate) fn restore(&mut self, snapshot: Value) {
        if !self.inner.has_shell {
            match serde_json::from_value::<Snapshot>(snapshot) {
                Ok(snapshot) if snapshot.version != SNAPSHOT_VERSION => {
                    self.report("snapshot", format!("a snapshot of version {} is not used: this build reads version {SNAPSHOT_VERSION}", snapshot.version));
                }
                Ok(snapshot) if snapshot.origin != self.config.origin => {
                    self.report(
                        "snapshot",
                        "a snapshot of another origin is not used".to_owned(),
                    );
                }
                Ok(snapshot) => self.inner.restore = Some(snapshot),
                Err(error) => self.report(
                    "snapshot",
                    format!("a snapshot that does not read is not used: {error}"),
                ),
            }
        }
        self.start();
    }

    /// The shell is here: the stacks of the snapshot are put back, when it
    /// was taken for the person the shell names. True when they were.
    pub(crate) fn restore_stacks(&mut self) -> bool {
        let Some(snapshot) = self.inner.restore.take() else {
            return false;
        };
        if snapshot.person != person_of(self.inner.user.as_ref()) {
            self.report(
                "snapshot",
                "a snapshot taken for another person is not used".to_owned(),
            );
            return false;
        }
        // Only onto a core that holds no screen yet.
        if !self.view.screens.is_empty() {
            return false;
        }
        let mut any = false;
        let put = |core: &mut Core, screens: Vec<KeptScreen>| -> Vec<ScreenId> {
            let mut stack = Vec::new();
            for screen in screens {
                // An address that is no longer the app's is left out.
                let crate::origin::Target::Inside(url) =
                    core.inner.origin.resolve(&screen.url, "/")
                else {
                    continue;
                };
                let id = core.new_screen(&url);
                if let (Some(view), Some(inner)) = (
                    core.view.screens.get_mut(&id),
                    core.inner.screens.get_mut(&id),
                ) {
                    view.title = screen.title;
                    view.from = screen.from;
                    // Fetched when it comes to the front.
                    inner.stale = true;
                    inner.restored = Some(Restored {
                        fields: screen.fields,
                        open: screen.open,
                    });
                }
                stack.push(id);
            }
            stack
        };
        for held in snapshot.destinations {
            let Some(at) = self
                .view
                .destinations
                .iter()
                .position(|destination| destination.href == held.href)
            else {
                continue;
            };
            let stack = put(self, held.stack);
            any |= !stack.is_empty();
            self.view.destinations[at].stack = stack;
        }
        if let Some(at) = snapshot.selected.and_then(|href| {
            self.view
                .destinations
                .iter()
                .position(|destination| destination.href == href)
        }) {
            self.view.selected = at;
        }
        for held in snapshot.modals {
            let present = match held.present {
                present @ (Present::Sheet | Present::Full | Present::Task) => present,
                _ => Present::Sheet,
            };
            let stack = put(self, held.stack);
            if !stack.is_empty() {
                any = true;
                self.view.modals.push(Modal {
                    present,
                    locked: false,
                    stack,
                });
            }
        }
        self.touch(Change::Nav);
        any
    }

    /// The page of a restored screen arrived: what the person had typed goes
    /// back into the fields that are still there and of the same kind, and
    /// the sheets that were open are opened.
    pub(crate) fn restore_typed(&mut self, id: ScreenId) {
        let Some(restored) = self
            .inner
            .screens
            .get_mut(&id)
            .and_then(|inner| inner.restored.take())
        else {
            return;
        };
        let (Some(view), Some(inner)) = (
            self.view.screens.get_mut(&id),
            self.inner.screens.get_mut(&id),
        ) else {
            return;
        };
        let ScreenView { content, forms, .. } = view;
        let crate::state::Content::Native { screen } = content else {
            return;
        };
        let mut inputs: BTreeMap<&str, &Input> = BTreeMap::new();
        for root in roots(screen) {
            walk(root, &mut |node| {
                if let Node::Field(field) = node
                    && let Some(id) = field.id.as_deref()
                {
                    inputs.insert(id, &field.input);
                }
            });
        }
        for (field, value) in restored.fields {
            let Some(input) = inputs.get(field.as_str()) else {
                continue;
            };
            if !fits(input, &value) || !may_keep(input, &value) {
                continue;
            }
            for (handle, form) in forms.iter_mut() {
                if let Some(held) = form.fields.get_mut(&field) {
                    held.value = value.clone();
                    inner.dirty.insert(field.clone());
                    self.changed.insert(Change::Form {
                        screen: id,
                        form: handle.clone(),
                    });
                    break;
                }
            }
        }
        for target in restored.open {
            self.set_open(id, &target, true);
        }
    }
}
