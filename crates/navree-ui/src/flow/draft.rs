//! [`Draft`]: where a person stands in a [`Wizard`](super::Wizard), kept on the
//! server. It belongs to a person, not to a tab: its id is in the address, so a link
//! resumes it anywhere, and two windows see the same draft.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};

/// The most bytes a draft holds, all its steps together.
pub const MAX_DRAFT_BYTES: usize = 256 * 1024;
/// The most bytes one value holds.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// The most values one step holds.
pub const MAX_STEP_VALUES: usize = 1000;
/// The most steps a draft records.
pub const MAX_STEPS: usize = 100;

/// What one step has been given.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    /// The fields as last posted, in order. Passwords and files are never here.
    pub values: Vec<(String, String)>,
    /// Whether the person has tried to move on from the step: its errors, if it
    /// has any, are shown from then on.
    pub visited: bool,
    /// Whether the step passed its rules when it was last tried, with everything
    /// that was posted. Only a step with a password in it relies on this, since
    /// the password is not kept to be checked again; any other step is checked
    /// again from its values each time.
    #[serde(default)]
    pub valid: bool,
}

/// A wizard in progress. It is plain data (`serde`), so an app can keep it in a
/// database column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    /// An opaque id: the address of the draft.
    pub id: String,
    /// Counts every change. A form carries the revision it was drawn from, and a
    /// post for an older one is refused: another window got there first.
    pub revision: u64,
    /// The step the person was last on.
    pub step: String,
    /// Whether the last step has been submitted. A submitted draft takes nothing
    /// more; the app creates what the wizard was for, then discards it.
    pub submitted: bool,
    /// What each step has been given, by the step's id.
    pub steps: BTreeMap<String, StepRecord>,
}

/// Whether `id` is something the toolkit would make: 8 to 64 letters, digits,
/// `-` and `_`. A draft is looked up by what a visitor typed, so this is checked
/// first.
pub fn is_draft_id(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether `id` is a step's id: 1 to 64 letters, digits, `-` and `_`.
pub fn is_step_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Draft {
    /// A new draft on `step`, with a fresh id.
    pub fn new(step: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            revision: 0,
            step: step.into(),
            submitted: false,
            steps: BTreeMap::new(),
        }
    }

    /// The record of a step.
    pub fn record(&self, step: &str) -> Option<&StepRecord> {
        self.steps.get(step)
    }

    /// The value posted for `field` in `step`: the first, when there are several.
    pub fn value(&self, step: &str, field: &str) -> Option<&str> {
        self.steps
            .get(step)?
            .values
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, value)| value.as_str())
    }

    /// A step read into the struct that is its form, with its rules run.
    pub fn read<T>(&self, step: &str) -> Result<T, crate::form::FieldErrors>
    where
        T: crate::form::Form + serde::de::DeserializeOwned + navree_validation::Validate,
    {
        let values = self.steps.get(step).map(|r| r.values.as_slice()).unwrap_or(&[]);
        crate::form::decode::<T>(values)
    }

    /// How many bytes the draft holds.
    pub fn bytes(&self) -> usize {
        self.id.len()
            + self.step.len()
            + self
                .steps
                .iter()
                .map(|(id, record)| {
                    id.len()
                        + record
                            .values
                            .iter()
                            .map(|(name, value)| name.len() + value.len())
                            .sum::<usize>()
                })
                .sum::<usize>()
    }

    /// Opens a submitted draft again, for an app whose creating of the record
    /// failed after the wizard said it was done.
    pub fn reopen(&mut self) {
        if self.submitted {
            self.submitted = false;
            self.revision += 1;
        }
    }
}

/// Why a store would not take or give a draft.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DraftError {
    /// The draft changed since it was read, or it already exists: read it again.
    Conflict,
    /// The draft is past [`MAX_DRAFT_BYTES`].
    TooLarge,
    /// The store has no room for another draft.
    Full,
    /// The store failed; the text says how, for the log and not for the person.
    Store(String),
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => f.write_str("the draft changed"),
            Self::TooLarge => f.write_str("the draft is too large"),
            Self::Full => f.write_str("there is no room for another draft"),
            Self::Store(why) => write!(f, "the draft store failed: {why}"),
        }
    }
}

impl std::error::Error for DraftError {}

/// Where drafts are kept. An `owner` is whoever the draft belongs to (a user id):
/// another owner's id finds nothing, so a draft's existence is not told to a
/// stranger.
pub trait DraftStore: Send + Sync {
    /// The draft `id` of `owner`.
    fn load(&self, owner: &str, id: &str) -> BoxFuture<'_, Result<Option<Draft>, DraftError>>;

    /// Keeps `draft`. With `expected` of `None` the draft must be new; with
    /// `Some(revision)` the stored one must still be at that revision. Otherwise
    /// it is a [`DraftError::Conflict`], so two writers never overwrite each
    /// other, and of two posts of Finish only one gets through.
    fn save(
        &self,
        owner: &str,
        draft: &Draft,
        expected: Option<u64>,
    ) -> BoxFuture<'_, Result<(), DraftError>>;

    /// Forgets the draft.
    fn discard(&self, owner: &str, id: &str) -> BoxFuture<'_, Result<(), DraftError>>;
}

/// Drafts in memory: for tests, the proof app and small apps with one process. It
/// holds at most `capacity` drafts and forgets one that nobody touched for `ttl`.
pub struct MemoryDrafts {
    capacity: usize,
    per_owner: usize,
    ttl: Duration,
    drafts: Mutex<HashMap<(String, String), (Draft, Instant)>>,
}

impl Default for MemoryDrafts {
    fn default() -> Self {
        Self::new(10_000, Duration::from_secs(7 * 24 * 3600))
    }
}

impl MemoryDrafts {
    /// A store of at most `capacity` drafts that forgets them after `ttl`. One
    /// owner may hold at most 100 of them.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            per_owner: 100,
            ttl,
            drafts: Mutex::new(HashMap::new()),
        }
    }

    /// How many drafts are held now.
    pub fn len(&self) -> usize {
        self.drafts.lock().map_or(0, |drafts| drafts.len())
    }

    /// Whether none is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn with<R>(&self, f: impl FnOnce(&mut HashMap<(String, String), (Draft, Instant)>) -> R) -> Result<R, DraftError> {
        let mut drafts = self
            .drafts
            .lock()
            .map_err(|_| DraftError::Store("the store's lock is poisoned".to_owned()))?;
        let now = Instant::now();
        let ttl = self.ttl;
        drafts.retain(|_, (_, touched)| now.duration_since(*touched) < ttl);
        Ok(f(&mut drafts))
    }
}

impl DraftStore for MemoryDrafts {
    fn load(&self, owner: &str, id: &str) -> BoxFuture<'_, Result<Option<Draft>, DraftError>> {
        let key = (owner.to_owned(), id.to_owned());
        Box::pin(async move {
            self.with(|drafts| {
                drafts.get_mut(&key).map(|(draft, touched)| {
                    *touched = Instant::now();
                    draft.clone()
                })
            })
        })
    }

    fn save(
        &self,
        owner: &str,
        draft: &Draft,
        expected: Option<u64>,
    ) -> BoxFuture<'_, Result<(), DraftError>> {
        let key = (owner.to_owned(), draft.id.clone());
        let draft = draft.clone();
        let owner = owner.to_owned();
        Box::pin(async move {
            if draft.bytes() > MAX_DRAFT_BYTES || !is_draft_id(&draft.id) {
                return Err(DraftError::TooLarge);
            }
            let (capacity, per_owner) = (self.capacity, self.per_owner);
            self.with(move |drafts| {
                match (drafts.get(&key), expected) {
                    (None, None) => {
                        if drafts.len() >= capacity
                            || drafts.keys().filter(|(who, _)| *who == owner).count() >= per_owner
                        {
                            return Err(DraftError::Full);
                        }
                    }
                    (Some((stored, _)), Some(revision)) if stored.revision == revision => {}
                    _ => return Err(DraftError::Conflict),
                }
                drafts.insert(key, (draft, Instant::now()));
                Ok(())
            })?
        })
    }

    fn discard(&self, owner: &str, id: &str) -> BoxFuture<'_, Result<(), DraftError>> {
        let key = (owner.to_owned(), id.to_owned());
        Box::pin(async move {
            self.with(|drafts| {
                drafts.remove(&key);
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        futures_executor_lite(future)
    }

    /// Runs a future that never waits on anything but a lock.
    fn futures_executor_lite<T>(future: impl std::future::Future<Output = T>) -> T {
        use std::task::{Context, Poll, Waker};
        let mut future = Box::pin(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    #[test]
    fn ids_are_checked_before_they_are_looked_up() {
        assert!(is_draft_id(&Draft::new("a").id));
        for bad in ["", "short", "has space  ", "../../etc/passwd", "a\0bcdefgh", &"x".repeat(65)] {
            assert!(!is_draft_id(bad), "{bad:?}");
        }
        assert!(is_step_id("basics-2_b"));
        assert!(!is_step_id("") && !is_step_id("a b") && !is_step_id(&"x".repeat(65)));
    }

    #[test]
    fn a_new_draft_has_a_fresh_id_and_reads_back_as_json() {
        let (a, b) = (Draft::new("one"), Draft::new("one"));
        assert_ne!(a.id, b.id);
        let mut d = a.clone();
        d.steps.insert(
            "one".into(),
            StepRecord { values: vec![("name".into(), "Ada \u{0} \"x\"".into())], visited: true, valid: true },
        );
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Draft>(&json).unwrap(), d);
    }

    #[test]
    fn the_store_is_a_compare_and_swap() {
        let store = MemoryDrafts::default();
        let mut draft = Draft::new("one");
        assert_eq!(block(store.save("ada", &draft, None)), Ok(()));
        // It exists now: saving it as new is a conflict.
        assert_eq!(block(store.save("ada", &draft, None)), Err(DraftError::Conflict));
        let before = draft.revision;
        draft.revision += 1;
        assert_eq!(block(store.save("ada", &draft, Some(before))), Ok(()));
        // A second writer holding the old revision loses.
        assert_eq!(block(store.save("ada", &draft, Some(before))), Err(DraftError::Conflict));
        assert_eq!(block(store.load("ada", &draft.id)).unwrap().unwrap().revision, 1);
    }

    #[test]
    fn another_owner_finds_nothing() {
        let store = MemoryDrafts::default();
        let draft = Draft::new("one");
        block(store.save("ada", &draft, None)).unwrap();
        assert_eq!(block(store.load("grace", &draft.id)), Ok(None));
        assert_eq!(block(store.save("grace", &draft, Some(0))), Err(DraftError::Conflict));
        assert_eq!(block(store.discard("grace", &draft.id)), Ok(()));
        assert!(block(store.load("ada", &draft.id)).unwrap().is_some());
        block(store.discard("ada", &draft.id)).unwrap();
        assert_eq!(block(store.load("ada", &draft.id)), Ok(None));
    }

    #[test]
    fn the_store_is_bounded() {
        let store = MemoryDrafts::new(2, Duration::from_secs(60));
        for _ in 0..2 {
            block(store.save("ada", &Draft::new("one"), None)).unwrap();
        }
        assert_eq!(block(store.save("ada", &Draft::new("one"), None)), Err(DraftError::Full));
        let mut big = Draft::new("one");
        big.steps.insert("one".into(), StepRecord { values: vec![("a".into(), "x".repeat(MAX_DRAFT_BYTES))], visited: false, valid: false });
        assert_eq!(block(MemoryDrafts::default().save("ada", &big, None)), Err(DraftError::TooLarge));
        let many = MemoryDrafts::default();
        for _ in 0..100 {
            block(many.save("ada", &Draft::new("one"), None)).unwrap();
        }
        assert_eq!(block(many.save("ada", &Draft::new("one"), None)), Err(DraftError::Full));
        assert!(block(many.save("grace", &Draft::new("one"), None)).is_ok());
    }

    #[test]
    fn a_draft_nobody_touched_is_forgotten() {
        let store = MemoryDrafts::new(10, Duration::from_millis(1));
        let draft = Draft::new("one");
        block(store.save("ada", &draft, None)).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(block(store.load("ada", &draft.id)), Ok(None));
        assert!(store.is_empty());
    }

    #[test]
    fn reopening_a_submitted_draft_bumps_the_revision() {
        let mut draft = Draft::new("one");
        draft.reopen();
        assert_eq!(draft.revision, 0);
        draft.submitted = true;
        draft.reopen();
        assert!(!draft.submitted);
        assert_eq!(draft.revision, 1);
    }
}
