//! A move, as the browser posts it, and the check that holds it against the lists
//! as the server has them now.

use http::HeaderValue;
use http::StatusCode;
use http::header::ALLOW;
use http::request::Parts;
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use navree_http::response::{IntoResponse, Response};

use crate::html::LocalHref;
use crate::op::{Envelope, Op, Reply, ToastTone};

/// The longest key, list or anchor a move may name, in bytes.
const MAX_KEY: usize = 256;

/// The highest position a move may ask for. Larger numbers mean the end.
const MAX_POSITION: u32 = 1_000_000;

/// The most name-and-value pairs read from one move's body.
const MAX_PAIRS: usize = 32;

/// One move of one item, as a [`Sortable`](crate::Sortable) posts it.
///
/// It reads a urlencoded form and answers forgery as [`Submit`](crate::Submit)
/// does: a body from another site gets 403 (unless its `Origin` is in
/// `http.cors.allowed_origins`), a body that isn't a form 415, and `GET` 405. A
/// body that names no item, no list, a key longer than 256 bytes or a position
/// that isn't a number gets 400.
///
/// Nothing in it is trusted: hold it against the lists as they are now, in the
/// transaction that writes the new order, with the lists it touches locked first,
/// so that two moves at once are applied one after the other and the second is
/// checked against what the first left. With the `orm` feature,
/// `Move::transaction` is that transaction: it begins at READ COMMITTED, locks the
/// lists as its first statement, and only then hands over the unit of work, in
/// which you [`check`](Self::check) (lists read whole) or [`place`](Self::place)
/// (lists kept by rank) the move and write it:
///
/// ```ignore
/// async fn moves(db: Db, mv: Move) -> Result<Reply> {
///     let checked = mv
///         .transaction(&db, &Column::KEY, |uow| {
///             let mv = mv.clone();
///             async move {
///                 let lists = board_lists(&uow).await?; // Vec<ListState>, under the lock
///                 let checked = mv.check(&lists);
///                 if let Ok(landing) = &checked {
///                     save_order(&uow, landing).await?;
///                 }
///                 Ok(checked)
///             }
///         })
///         .await?;
///     Ok(match checked {
///         Ok(_) => mv.done("/board"),
///         Err(refusal) => mv.refused("/board", refusal.message()),
///     })
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Move {
    /// The key of the item that moves.
    pub item: String,
    /// The list the browser saw it in. If it is in another list now, someone moved
    /// it first, and the move is stale.
    pub from_list: String,
    /// The list it goes to: the same list to reorder it.
    pub to_list: String,
    /// The item it goes before in that list.
    pub before: Option<String>,
    /// The item it goes after, when it was dropped below the last item the browser
    /// had: a long list the page holds only the start of. Read when `before` is
    /// `None`; with neither, it goes after the last one.
    pub after: Option<String>,
    /// Where it goes by number, 1 being first, from the form a person fills in with
    /// JavaScript off. Read only when `before` and `after` are `None`.
    pub position: Option<u32>,
}

impl Move {
    /// A move of `item` from `from_list` to `to_list`, at the end.
    pub fn new(
        item: impl Into<String>,
        from_list: impl Into<String>,
        to_list: impl Into<String>,
    ) -> Self {
        Self {
            item: item.into(),
            from_list: from_list.into(),
            to_list: to_list.into(),
            before: None,
            after: None,
            position: None,
        }
    }

    /// Puts it before `item`.
    #[must_use]
    pub fn before(mut self, item: impl Into<String>) -> Self {
        self.before = Some(item.into());
        self
    }

    /// Puts it after `item`.
    #[must_use]
    pub fn after(mut self, item: impl Into<String>) -> Self {
        self.after = Some(item.into());
        self
    }

    /// Puts it at `position`, 1 being first.
    #[must_use]
    pub fn at(mut self, position: u32) -> Self {
        self.position = Some(position);
        self
    }

    /// The lists the move touches, sorted and each once: the lists to lock, in the
    /// order to lock them in, so that two moves never wait for each other.
    ///
    /// ```
    /// use navree_ui::Move;
    ///
    /// assert_eq!(Move::new("12", "ready", "doing").lists(), ["doing", "ready"]);
    /// assert_eq!(Move::new("12", "ready", "ready").lists(), ["ready"]);
    /// ```
    pub fn lists(&self) -> Vec<&str> {
        let mut lists = vec![self.from_list.as_str(), self.to_list.as_str()];
        lists.sort_unstable();
        lists.dedup();
        lists
    }

    /// Reads a move from a urlencoded body. The first value of each field counts;
    /// `before`, `after` and `position` may be empty.
    ///
    /// A native client posts a move in its own words: `from` and `to` for the lists,
    /// the ids of the page's nodes for the item, the lists and `before`, a `position`
    /// counted from 0, and `_sortable` for the sortable the ids belong to. They are
    /// read back as the keys they were made from.
    ///
    /// # Errors
    ///
    /// When a field is missing or empty, a key is longer than 256 bytes, the
    /// position isn't a whole number, or the body holds more than 32 pairs.
    pub fn from_form(body: &[u8]) -> Result<Self, MoveError> {
        let mut item = None;
        let mut from_list = None;
        let mut to_list = None;
        let mut before = None;
        let mut after = None;
        let mut position = None;
        let mut native_from = None;
        let mut native_to = None;
        let mut sortable = None;
        for (count, (name, value)) in urlencoded::parse(body).enumerate() {
            if count >= MAX_PAIRS {
                return Err(MoveError::TooLarge);
            }
            let slot = match name.as_ref() {
                "item" => &mut item,
                "from_list" => &mut from_list,
                "to_list" => &mut to_list,
                "before" => &mut before,
                "after" => &mut after,
                "position" => &mut position,
                "from" => &mut native_from,
                "to" => &mut native_to,
                "_sortable" => &mut sortable,
                _ => continue,
            };
            if slot.is_none() {
                *slot = Some(value.into_owned());
            }
        }
        // The ids a native client names are the page's: `<sortable>-item-<key>`, and
        // `<sortable>-list-<key>` for a list.
        let native = sortable.filter(|id| !id.is_empty());
        let (item, from_list, to_list, before) = match &native {
            Some(root) => {
                let of = |value: Option<String>, kind: &str, name: &'static str| match value {
                    Some(value) if !value.is_empty() => value
                        .strip_prefix(&format!("{root}-{kind}-"))
                        // A list is the node of its items.
                        .map(|key| match kind {
                            "list" => key.strip_suffix("-items").unwrap_or(key),
                            _ => key,
                        })
                        .map(str::to_owned)
                        .ok_or(MoveError::Field(name)),
                    other => Ok(other.unwrap_or_default()),
                };
                (
                    of(item, "item", "item")?,
                    of(native_from, "list", "from")?,
                    of(native_to, "list", "to")?,
                    of(before, "item", "before")?,
                )
            }
            None => (
                item.unwrap_or_default(),
                from_list.unwrap_or_default(),
                to_list.unwrap_or_default(),
                before.unwrap_or_default(),
            ),
        };
        let (item, from_list, to_list, before) = (
            Some(item),
            Some(from_list),
            Some(to_list),
            Some(before),
        );
        let key = |value: Option<String>, name: &'static str| match value {
            Some(value) if !value.is_empty() && value.len() <= MAX_KEY => Ok(value),
            _ => Err(MoveError::Field(name)),
        };
        let anchor = |value: Option<String>, name: &'static str| match value {
            Some(value) if value.len() > MAX_KEY => Err(MoveError::Field(name)),
            Some(value) if !value.is_empty() => Ok(Some(value)),
            _ => Ok(None),
        };
        let before = anchor(before, "before")?;
        let after = anchor(after, "after")?;
        let position = match position.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(text) => {
                if text.len() > 12 || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(MoveError::Field("position"));
                }
                // A native client counts from 0, a form from 1.
                let shift = u64::from(native.is_some());
                Some(text.parse::<u64>().map_or(MAX_POSITION, |n| {
                    u32::try_from((n + shift).min(u64::from(MAX_POSITION))).unwrap_or(MAX_POSITION)
                }))
            }
        };
        Ok(Self {
            item: key(item, "item")?,
            from_list: key(from_list, "from_list")?,
            to_list: key(to_list, "to_list")?,
            before,
            after,
            position,
        })
    }

    /// Holds the move against `lists`, the lists as the server has them now, and
    /// says where the item lands, or why it can't move.
    ///
    /// # Errors
    ///
    /// A [`Refusal`]: the item or the list is gone, the item is no longer in the
    /// list the browser saw it in, the item it was dropped before has left that
    /// list, the list is full or takes no items from others.
    pub fn check(&self, lists: &[ListState]) -> Result<Landing, Refusal> {
        let (from_at, from_index) = lists
            .iter()
            .enumerate()
            .find_map(|(at, list)| {
                list.items
                    .iter()
                    .position(|key| *key == self.item)
                    .map(|index| (at, index))
            })
            .ok_or(Refusal::Gone)?;
        let from = &lists[from_at];
        if from.key != self.from_list {
            return Err(Refusal::Stale {
                list: from.title.clone(),
            });
        }
        let to_at = lists
            .iter()
            .position(|list| list.key == self.to_list)
            .ok_or(Refusal::NoList)?;
        if self.before.as_deref() == Some(self.item.as_str())
            || self.after.as_deref() == Some(self.item.as_str())
        {
            return Err(Refusal::Malformed);
        }
        let target = &lists[to_at];
        let same = to_at == from_at;
        if !same {
            if let Some(reason) = &target.refuse {
                return Err(Refusal::Refused {
                    reason: reason.clone(),
                });
            }
            if let Some(limit) = target.limit
                && target.items.len() >= limit as usize
            {
                return Err(Refusal::Full {
                    list: target.title.clone(),
                    limit,
                });
            }
        }
        let mut order: Vec<String> = target
            .items
            .iter()
            .filter(|key| **key != self.item)
            .cloned()
            .collect();
        let anchored = |anchor: &str| {
            order
                .iter()
                .position(|key| key == anchor)
                .ok_or(Refusal::Moved)
        };
        let index = match (&self.before, &self.after, self.position) {
            (Some(anchor), _, _) => anchored(anchor)?,
            (None, Some(anchor), _) => anchored(anchor)? + 1,
            (None, None, Some(position)) => usize::try_from(position.saturating_sub(1))
                .unwrap_or(usize::MAX)
                .min(order.len()),
            (None, None, None) => order.len(),
        };
        order.insert(index, self.item.clone());
        let left = if same {
            Vec::new()
        } else {
            from.items
                .iter()
                .filter(|key| **key != self.item)
                .cloned()
                .collect()
        };
        Ok(Landing {
            item: self.item.clone(),
            from_list: from.key.clone(),
            to_list: target.key.clone(),
            index,
            order,
            left,
            moved: !(same && from_index == index),
        })
    }

    /// The answer to a move that was made. The runtime has moved the item already,
    /// so it gets no ops; with JavaScript off the browser goes back to `page`.
    pub fn done(&self, page: impl Into<LocalHref>) -> Reply {
        Reply::ops_or_redirect(page.into().into_href())
    }

    /// The answer to a refused move: `409 Conflict` and a toast that says `why`.
    /// The runtime puts the item back where it was; with JavaScript off the browser
    /// goes back to `page`, which shows the toast. Add a morph of the lists when
    /// the refusal means the page is out of date.
    pub fn refused(&self, page: impl Into<LocalHref>, why: impl Into<String>) -> Reply {
        Reply::ops_or_redirect(page.into().into_href())
            .toast(ToastTone::Warning, why)
            .status(StatusCode::CONFLICT)
    }
}

/// Why a body isn't a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoveError {
    /// A field is missing, empty, too long, or not a number where one is wanted.
    Field(&'static str),
    /// The body holds more pairs than a move has.
    TooLarge,
}

/// A request that isn't a move: what [`Move`] answers in its place.
#[derive(Debug)]
pub struct MoveRejected {
    status: StatusCode,
    asked: crate::asked::WhoAsked,
}

impl MoveRejected {
    /// The status it answers with.
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for MoveRejected {
    fn into_response(self) -> Response {
        if self.status == StatusCode::BAD_REQUEST
            && matches!(
                self.asked,
                crate::asked::WhoAsked::Runtime | crate::asked::WhoAsked::Native
            )
        {
            let ops = vec![Op::toast(
                ToastTone::Danger,
                "That move couldn't be read. Reload the page and try again.",
            )];
            // A native client is told the same, as a native op.
            #[cfg(feature = "native-preview")]
            if self.asked.is_native()
                && let Some(response) = crate::native::swap::ops(self.status, &ops)
            {
                return response;
            }
            if self.asked.is_runtime() {
                return (self.status, Envelope::new(ops)).into_response();
            }
        }
        let mut response = self.status.into_response();
        if self.status == StatusCode::METHOD_NOT_ALLOWED {
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("POST"));
        }
        response
    }
}

impl FromRequest for Move {
    type Rejection = MoveRejected;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body): (Parts, _) = request.into_parts();
        let asked = crate::asked::who_asked(&parts.headers);
        let refuse = |status| MoveRejected { status, asked };
        if let Some(status) = crate::form::refusal(&parts).await {
            return Err(refuse(status));
        }
        let limit = BodyLimit::of(&parts.extensions);
        let Ok(bytes) = read_body(body, limit).await else {
            return Err(refuse(StatusCode::PAYLOAD_TOO_LARGE));
        };
        match Self::from_form(&bytes) {
            Ok(mv) => Ok(mv),
            Err(MoveError::TooLarge) => Err(refuse(StatusCode::PAYLOAD_TOO_LARGE)),
            Err(MoveError::Field(_)) => Err(refuse(StatusCode::BAD_REQUEST)),
        }
    }
}

/// One list as the server has it now, to check a [`Move`] against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct ListState {
    /// Its key.
    pub key: String,
    /// Its name, for the reasons a move is refused.
    pub title: String,
    /// The keys of its items, in order.
    pub items: Vec<String>,
    /// The most items it holds.
    pub limit: Option<u32>,
    /// Why it takes no item from another list, when it doesn't.
    pub refuse: Option<String>,
}

impl ListState {
    /// The list `key`, called `title`, holding `items` in this order.
    pub fn new<I, K>(key: impl Into<String>, title: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        Self {
            key: key.into(),
            title: title.into(),
            items: items.into_iter().map(Into::into).collect(),
            limit: None,
            refuse: None,
        }
    }

    /// The most items it holds: a move from another list into a list this full is
    /// refused.
    pub fn limit(mut self, limit: impl Into<Option<u32>>) -> Self {
        self.limit = limit.into();
        self
    }

    /// Takes no item from another list, because of `reason`.
    pub fn refuse(mut self, reason: impl Into<String>) -> Self {
        self.refuse = Some(reason.into());
        self
    }

    /// Takes no item from another list when `refused` is true.
    pub fn refuse_if(self, refused: bool, reason: impl Into<String>) -> Self {
        if refused { self.refuse(reason) } else { self }
    }
}

/// Where a checked move puts the item.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Landing {
    /// The item.
    pub item: String,
    /// The list it left.
    pub from_list: String,
    /// The list it is in now.
    pub to_list: String,
    /// Its place in that list, 0 being first.
    pub index: usize,
    /// That list's items in their new order.
    pub order: Vec<String>,
    /// The order of the list it left, when it left one; empty when it stayed in its
    /// list.
    pub left: Vec<String>,
    /// Whether it moved at all: a drop where it was is a landing that changes nothing.
    pub moved: bool,
}

/// Why a move was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The item is in no list: it was deleted.
    Gone,
    /// The list it goes to doesn't exist.
    NoList,
    /// The item is no longer in the list the browser saw it in: someone moved it.
    /// `list` names where it is now.
    Stale {
        /// Where it is now.
        list: String,
    },
    /// The item it was dropped before, or after, has left the list since the page
    /// was drawn.
    Moved,
    /// The list is full.
    Full {
        /// The list's name.
        list: String,
        /// The most it holds.
        limit: u32,
    },
    /// The list takes no item from another.
    Refused {
        /// Why, as the list says it.
        reason: String,
    },
    /// The move makes no sense: an item before or after itself.
    Malformed,
}

impl Refusal {
    /// What to tell the person, in English.
    pub fn message(&self) -> String {
        match self {
            Self::Gone => "That item isn't here any more.".to_owned(),
            Self::NoList => "That list isn't here any more.".to_owned(),
            Self::Stale { list } => {
                format!("Someone else moved it first. It is in {list} now.")
            }
            Self::Moved => "The list changed while you were moving it. Try again.".to_owned(),
            Self::Full { list, limit } => format!("{list} holds at most {limit}."),
            Self::Refused { reason } => reason.clone(),
            Self::Malformed => "That move couldn't be read.".to_owned(),
        }
    }

    /// Whether the page that sent the move is out of date: worth redrawing the
    /// lists in the refusal.
    pub fn is_stale(&self) -> bool {
        matches!(
            self,
            Self::Gone | Self::NoList | Self::Stale { .. } | Self::Moved
        )
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}
