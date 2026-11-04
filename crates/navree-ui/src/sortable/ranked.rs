//! Moves in lists too long to read whole: each item has a rank, a number, and a
//! list's order is its ranks' order. A move reads only what it needs (where the
//! item is, the list's size and rules, the items beside where it goes) and gives
//! the item a rank between theirs, so it writes one row.

use std::future::Future;

use super::moves::{ListState, Move, Refusal};

/// The room left between two ranks a new list starts with, and that an item put
/// first or last keeps from its neighbour.
pub const RANK_GAP: i64 = 1 << 16;

/// Where an item is in a ranked list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    /// The list's key.
    pub list: String,
    /// Its rank.
    pub rank: i64,
}

/// What [`Move::place`] reads, in the move's transaction, from lists kept in order
/// by rank. Each method leaves `item`, the one that moves, out of what it counts
/// and finds.
pub trait RankedLists {
    /// What the store fails with.
    type Error;

    /// Where `key` is now, or `None` when it is gone.
    fn spot(&self, key: &str) -> impl Future<Output = Result<Option<Spot>, Self::Error>> + Send;

    /// The list `key` (its title, limit and refusal; its `items` are not read) and
    /// how many items it holds, `item` left out. `None` when there is no such list.
    fn list(
        &self,
        key: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<(ListState, u64)>, Self::Error>> + Send;

    /// The rank of the item next to `rank` in `list`: the nearest lower rank when
    /// `lower`, otherwise the nearest higher one.
    fn beside(
        &self,
        list: &str,
        rank: i64,
        lower: bool,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, Self::Error>> + Send;

    /// The rank of the item at `index` in `list`, 0 being first.
    fn nth(
        &self,
        list: &str,
        index: u64,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, Self::Error>> + Send;

    /// The highest rank in `list`.
    fn last(
        &self,
        list: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, Self::Error>> + Send;
}

/// Where a placed move puts the item.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Placement {
    /// The item.
    pub item: String,
    /// The list it left.
    pub from_list: String,
    /// The list it is in now.
    pub to_list: String,
    /// Its new rank. `None` when its neighbours' ranks are next to each other: give
    /// the list fresh ranks [`RANK_GAP`] apart, then place the move again.
    pub rank: Option<i64>,
    /// Whether it moved at all.
    pub moved: bool,
}

/// A rank between `lower` and `upper`, either of which may be missing (the start or
/// the end of the list). `None` when there is no whole number between them.
///
/// ```
/// use navree_ui::sortable::{RANK_GAP, rank_between};
///
/// assert_eq!(rank_between(None, None), Some(0));
/// assert_eq!(rank_between(Some(10), None), Some(10 + RANK_GAP));
/// assert_eq!(rank_between(Some(10), Some(20)), Some(15));
/// assert_eq!(rank_between(Some(10), Some(11)), None);
/// ```
pub fn rank_between(lower: Option<i64>, upper: Option<i64>) -> Option<i64> {
    match (lower, upper) {
        (None, None) => Some(0),
        (Some(low), None) => low.checked_add(RANK_GAP),
        (None, Some(high)) => high.checked_sub(RANK_GAP),
        (Some(low), Some(high)) => {
            let room = high.checked_sub(low)?;
            (room >= 2).then(|| low + room / 2)
        }
    }
}

impl Move {
    /// Holds the move against lists kept by rank, reading only what it needs, and
    /// says where the item goes, or why it can't. The checks are those of
    /// [`check`](Self::check): the item is where the browser saw it, its anchor is
    /// still in the list it goes to, and that list has room and takes items.
    ///
    /// Run it in the transaction that writes the rank, after locking the lists it
    /// touches (SQLite's `BEGIN IMMEDIATE`; `SELECT ... FOR UPDATE` on a row per list
    /// or an advisory lock on PostgreSQL and MySQL), so two moves at once are placed
    /// one after the other.
    ///
    /// # Errors
    ///
    /// The store's own error, when a read fails. A refusal is the inner `Err`.
    pub async fn place<L: RankedLists + Sync>(
        &self,
        lists: &L,
    ) -> Result<Result<Placement, Refusal>, L::Error> {
        let item = self.item.as_str();
        let Some(spot) = lists.spot(item).await? else {
            return Ok(Err(Refusal::Gone));
        };
        if spot.list != self.from_list {
            let title = match lists.list(&spot.list, item).await? {
                Some((list, _)) => list.title,
                None => spot.list.clone(),
            };
            return Ok(Err(Refusal::Stale { list: title }));
        }
        let Some((target, count)) = lists.list(&self.to_list, item).await? else {
            return Ok(Err(Refusal::NoList));
        };
        if self.before.as_deref() == Some(item) || self.after.as_deref() == Some(item) {
            return Ok(Err(Refusal::Malformed));
        }
        let same = spot.list == self.to_list;
        if !same {
            if let Some(reason) = target.refuse {
                return Ok(Err(Refusal::Refused { reason }));
            }
            if let Some(limit) = target.limit
                && count >= u64::from(limit)
            {
                return Ok(Err(Refusal::Full {
                    list: target.title,
                    limit,
                }));
            }
        }
        let to = self.to_list.as_str();
        let (lower, upper) = match (&self.before, &self.after, self.position) {
            (Some(anchor), _, _) => match lists.spot(anchor).await? {
                Some(at) if at.list == to => {
                    (lists.beside(to, at.rank, true, item).await?, Some(at.rank))
                }
                _ => return Ok(Err(Refusal::Moved)),
            },
            (None, Some(anchor), _) => match lists.spot(anchor).await? {
                Some(at) if at.list == to => {
                    (Some(at.rank), lists.beside(to, at.rank, false, item).await?)
                }
                _ => return Ok(Err(Refusal::Moved)),
            },
            (None, None, Some(position)) => {
                let index = u64::from(position.saturating_sub(1)).min(count);
                let lower = match index {
                    0 => None,
                    _ => lists.nth(to, index - 1, item).await?,
                };
                (lower, lists.nth(to, index, item).await?)
            }
            (None, None, None) => (lists.last(to, item).await?, None),
        };
        let stays = same
            && lower.is_none_or(|low| low < spot.rank)
            && upper.is_none_or(|high| spot.rank < high);
        Ok(Ok(Placement {
            item: self.item.clone(),
            from_list: spot.list,
            to_list: self.to_list.clone(),
            rank: if stays {
                Some(spot.rank)
            } else {
                rank_between(lower, upper)
            },
            moved: !stays,
        }))
    }
}
