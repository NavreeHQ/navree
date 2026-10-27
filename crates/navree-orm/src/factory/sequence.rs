//! [`Sequence`]: states taken in turn by the records of a batch.

use std::fmt;
use std::sync::Arc;

use crate::column::{Column, ColumnName};
use crate::model::Model;
use crate::value::{Field, ValueFor};

/// Where a record stands in a [`Sequence`]: its position in the batch and
/// the number of states the sequence cycles through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequenceIndex {
    index: usize,
    count: usize,
}

impl SequenceIndex {
    /// The record's position in the batch, from 0.
    pub fn index(&self) -> usize {
        self.index
    }

    /// How many states the sequence has (`0` for a sequence computed by a
    /// function).
    pub fn count(&self) -> usize {
        self.count
    }
}

type Step<M> = Arc<dyn Fn(&mut M, SequenceIndex) -> crate::Result<()> + Send + Sync>;

/// States the records of a batch take in turn: the first record gets the
/// first, the second the second, and so on, starting over after the last.
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_orm::factory::Sequence;
///
/// // Fixed states (functions or closures without captures).
/// let stages = Sequence::new([|d: &mut Deal| d.stage = Stage::Open, |d: &mut Deal| d.stage = Stage::Won]);
/// // The values of one column.
/// let titles = Sequence::column(Deal::TITLE, ["Renewal", "Upsell", "Pilot"]);
/// // A function of the position.
/// let ranked = Sequence::from_fn(|d: &mut Deal, at| d.amount_cents = 1_000 * (at.index() as i64 + 1));
/// # let _ = (stages, titles, ranked);
/// ```
pub struct Sequence<M> {
    steps: Vec<Step<M>>,
    computed: Option<Step<M>>,
}

impl<M: Model> Sequence<M> {
    /// The states `states`, in turn.
    pub fn new<const N: usize>(states: [fn(&mut M); N]) -> Self {
        Sequence {
            steps: states
                .into_iter()
                .map(|state| {
                    Arc::new(move |model: &mut M, _: SequenceIndex| {
                        state(model);
                        Ok(())
                    }) as Step<M>
                })
                .collect(),
            computed: None,
        }
    }

    /// `column` set to each of `values` in turn.
    pub fn column<T, V>(column: Column<M, T>, values: impl IntoIterator<Item = V>) -> Self
    where
        V: ValueFor<T>,
        T: Field,
    {
        let name = column.column_name().into_owned();
        let steps = values
            .into_iter()
            .map(|value| {
                let encoded = column.encode(&value.into_value_for());
                let name = name.clone();
                Arc::new(move |model: &mut M, _: SequenceIndex| match &encoded {
                    Ok(value) => model.set_value(&name, value.clone()),
                    Err(error) => Err(crate::OrmError::factory(
                        M::meta().name(),
                        format!("cannot set `{name}` from a sequence: {error}"),
                    )),
                }) as Step<M>
            })
            .collect();
        Sequence {
            steps,
            computed: None,
        }
    }

    /// A state computed from each record's position (`at.index()`): the
    /// sequence never repeats.
    pub fn from_fn(state: impl Fn(&mut M, SequenceIndex) + Send + Sync + 'static) -> Self {
        Sequence {
            steps: Vec::new(),
            computed: Some(Arc::new(move |model, at| {
                state(model, at);
                Ok(())
            })),
        }
    }

    /// Every combination of one state of each sequence in `sequences`,
    /// applied in order: `[a, b]` × `[x, y]` gives `a+x`, `a+y`, `b+x`,
    /// `b+y`.
    pub fn cross_join(sequences: impl IntoIterator<Item = Sequence<M>>) -> Self {
        let mut combinations: Vec<Vec<Step<M>>> = vec![Vec::new()];
        for sequence in sequences {
            let steps = if sequence.steps.is_empty() {
                sequence.computed.into_iter().collect()
            } else {
                sequence.steps
            };
            if steps.is_empty() {
                continue;
            }
            combinations = combinations
                .into_iter()
                .flat_map(|prefix| {
                    steps.iter().map(move |step| {
                        let mut combined = prefix.clone();
                        combined.push(step.clone());
                        combined
                    })
                })
                .collect();
        }
        Sequence {
            steps: combinations
                .into_iter()
                .filter(|combination| !combination.is_empty())
                .map(|combination| {
                    Arc::new(move |model: &mut M, at: SequenceIndex| {
                        for step in &combination {
                            step(model, at)?;
                        }
                        Ok(())
                    }) as Step<M>
                })
                .collect(),
            computed: None,
        }
    }

    /// How many states the sequence cycles through (`0` for
    /// [`from_fn`](Sequence::from_fn)).
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the sequence has no fixed states (an empty or computed
    /// sequence).
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Applies the state of the record at `index`.
    pub(crate) fn apply(&self, model: &mut M, index: usize) -> crate::Result<()> {
        let count = self.steps.len();
        let at = SequenceIndex { index, count };
        if let Some(computed) = &self.computed {
            return computed(model, at);
        }
        match count {
            0 => Ok(()),
            _ => (self.steps[index % count])(model, at),
        }
    }
}

impl<M> Clone for Sequence<M> {
    fn clone(&self) -> Self {
        Sequence {
            steps: self.steps.clone(),
            computed: self.computed.clone(),
        }
    }
}

impl<M> fmt::Debug for Sequence<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sequence")
            .field("states", &self.steps.len())
            .field("computed", &self.computed.is_some())
            .finish()
    }
}

impl<M: Model, const N: usize> From<[fn(&mut M); N]> for Sequence<M> {
    fn from(states: [fn(&mut M); N]) -> Self {
        Sequence::new(states)
    }
}
