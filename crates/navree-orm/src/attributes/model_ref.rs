//! Records in queued jobs: [`ModelRef`] and [`ModelRefs`] carry a record's
//! identity (its model, key, loaded relation names and connection) instead
//! of its values, and load it again when the job runs, so the job sees the
//! record as it is then.
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use navree_orm::attributes::ModelRef;
//! use serde::{Deserialize, Serialize};
//!
//! /// Emails a deal's owner (serialized into the queue).
//! #[derive(Serialize, Deserialize)]
//! struct NotifyOwner {
//!     deal: ModelRef<Deal>,
//! }
//!
//! # async fn demo(uow: Uow, deal: Record<Deal>) -> Result<()> {
//! let job = NotifyOwner { deal: deal.to_ref()? };
//! // ... later, when the job runs:
//! let deal = job.deal.restore(&uow).await?; // 404-shaped ModelNotFound if it is gone
//! # Ok(()) }
//! ```

use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::collection::Collection;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::model::{Model, PrimaryKey};
use crate::query::Builder;
use crate::record::Record;
use crate::value::{Field, to_json};

/// One record's identity, for a queued job. Serializes as `{"model":
/// "Deal", "key": ..., "relations": [..], "connection": ..}`.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct ModelRef<M> {
    model: String,
    key: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connection: Option<String>,
    #[serde(skip)]
    _model: PhantomData<fn() -> M>,
}

/// Several records' identities, for a queued job; restored in the same
/// order.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
pub struct ModelRefs<M> {
    model: String,
    keys: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connection: Option<String>,
    #[serde(skip)]
    _model: PhantomData<fn() -> M>,
}

fn check_model<M: Model>(model: &str) -> Result<()> {
    if model == M::meta().name() {
        Ok(())
    } else {
        Err(OrmError::InvalidQuery(format!(
            "this reference names model {model}, not {}",
            M::meta().name()
        )))
    }
}

fn key_from<M: Model>(json: &serde_json::Value) -> Result<M::Key> {
    M::Key::from_json(json.clone()).map_err(|error| OrmError::Decode {
        model: M::meta().name().into(),
        column: M::meta().key_column().to_owned(),
        expected: M::Key::type_name(),
        message: format!("the reference's key {json}: {error}"),
    })
}

impl<M: Model> ModelRef<M> {
    /// The identity of `record`, which must be stored.
    pub fn of(record: &Record<M>) -> Result<Self> {
        if !record.exists() {
            return Err(Record::<M>::not_persisted("refer to"));
        }
        Ok(ModelRef {
            model: M::meta().name().to_owned(),
            key: to_json(&record.key().to_value()),
            relations: record.relations().names().map(str::to_owned).collect(),
            connection: M::meta().connection_name().map(str::to_owned),
            _model: PhantomData,
        })
    }

    /// The model's name the reference was made for.
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// The record's key.
    pub fn key(&self) -> Result<M::Key> {
        check_model::<M>(&self.model)?;
        key_from::<M>(&self.key)
    }

    /// The names of the relations the record had loaded, to load again
    /// with the restored record.
    pub fn relations(&self) -> &[String] {
        &self.relations
    }

    /// The model's connection when the reference was made.
    pub fn connection(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    /// The record as it is now, read through the writer and without global
    /// scopes (a soft-deleted record restores too);
    /// [`OrmError::ModelNotFound`] when it is gone, which fails the job
    /// with a clear message.
    pub async fn restore<E: Executor + ?Sized>(&self, exec: &E) -> Result<Record<M>> {
        let key = self.key()?;
        self.restore_or_skip(exec)
            .await?
            .ok_or_else(|| OrmError::not_found(M::meta().name(), vec![key.to_key_string()]))
    }

    /// Like [`restore`](ModelRef::restore), with `None` when the record is
    /// gone: for jobs that skip records deleted since they were queued.
    pub async fn restore_or_skip<E: Executor + ?Sized>(
        &self,
        exec: &E,
    ) -> Result<Option<Record<M>>> {
        let key = self.key()?;
        let target = exec.target(Access::Write).await?;
        Ok(Builder::<M>::new()
            .without_global_scopes()
            .where_key(key)
            .limit(1)
            .fetch(&target)
            .await?
            .into_vec()
            .into_iter()
            .next())
    }
}

impl<M: Model> ModelRefs<M> {
    /// The identities of `records`, which must all be stored.
    pub fn of(records: &Collection<M>) -> Result<Self> {
        let mut keys = Vec::with_capacity(records.len());
        let mut relations: Vec<String> = Vec::new();
        for record in records.iter() {
            if !record.exists() {
                return Err(Record::<M>::not_persisted("refer to"));
            }
            keys.push(to_json(&record.key().to_value()));
            for name in record.relations().names() {
                if !relations.iter().any(|known| known == name) {
                    relations.push(name.to_owned());
                }
            }
        }
        Ok(ModelRefs {
            model: M::meta().name().to_owned(),
            keys,
            relations,
            connection: M::meta().connection_name().map(str::to_owned),
            _model: PhantomData,
        })
    }

    /// The model's name the references were made for.
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// The records' keys, in order.
    pub fn keys(&self) -> Result<Vec<M::Key>> {
        check_model::<M>(&self.model)?;
        self.keys.iter().map(key_from::<M>).collect()
    }

    /// How many records.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The names of the relations the records had loaded.
    pub fn relations(&self) -> &[String] {
        &self.relations
    }

    /// The models' connection when the references were made.
    pub fn connection(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    /// The records as they are now, in the references' order, read through
    /// the writer and without global scopes; records gone since are left
    /// out.
    pub async fn restore<E: Executor + ?Sized>(&self, exec: &E) -> Result<Collection<M>> {
        let keys = self.keys()?;
        if keys.is_empty() {
            return Ok(Collection::new());
        }
        let target = exec.target(Access::Write).await?;
        let mut found = Builder::<M>::new()
            .without_global_scopes()
            .where_key_in(keys.clone())
            .fetch(&target)
            .await?
            .into_vec();
        let mut ordered = Vec::with_capacity(found.len());
        for key in keys {
            if let Some(index) = found.iter().position(|record| record.key() == key) {
                ordered.push(found.swap_remove(index));
            }
        }
        Ok(ordered.into())
    }
}

impl<M: Model> Record<M> {
    /// The record's identity, for a queued job ([`ModelRef`]).
    pub fn to_ref(&self) -> Result<ModelRef<M>> {
        ModelRef::of(self)
    }
}

impl<M: Model> Collection<M> {
    /// The records' identities, for a queued job ([`ModelRefs`]).
    pub fn to_refs(&self) -> Result<ModelRefs<M>> {
        ModelRefs::of(self)
    }
}

impl<M> Clone for ModelRef<M> {
    fn clone(&self) -> Self {
        ModelRef {
            model: self.model.clone(),
            key: self.key.clone(),
            relations: self.relations.clone(),
            connection: self.connection.clone(),
            _model: PhantomData,
        }
    }
}

impl<M> Clone for ModelRefs<M> {
    fn clone(&self) -> Self {
        ModelRefs {
            model: self.model.clone(),
            keys: self.keys.clone(),
            relations: self.relations.clone(),
            connection: self.connection.clone(),
            _model: PhantomData,
        }
    }
}

impl<M> PartialEq for ModelRef<M> {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model
            && self.key == other.key
            && self.relations == other.relations
            && self.connection == other.connection
    }
}

impl<M> PartialEq for ModelRefs<M> {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model
            && self.keys == other.keys
            && self.relations == other.relations
            && self.connection == other.connection
    }
}

impl<M> fmt::Debug for ModelRef<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelRef")
            .field("model", &self.model)
            .field("key", &self.key)
            .field("relations", &self.relations)
            .field("connection", &self.connection)
            .finish()
    }
}

impl<M> fmt::Debug for ModelRefs<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelRefs")
            .field("model", &self.model)
            .field("keys", &self.keys)
            .field("relations", &self.relations)
            .field("connection", &self.connection)
            .finish()
    }
}
