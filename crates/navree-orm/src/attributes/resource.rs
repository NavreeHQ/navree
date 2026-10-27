//! Records as API resources: the shapes front ends receive, with the
//! `__type` subject navree-api's resources carry for permission checks,
//! and field-level masks.
//!
//! A record is a resource of its own (its serialized JSON plus `"__type":
//! "<Model>"`, [`Record::to_resource_json`]), or becomes a dedicated
//! resource type ([`Resource`], [`Record::to_resource`]); a model names its
//! resource with `#[model(resource = DealResource)]`
//! ([`Record::resource`]). Masks keep the fields an ability permits
//! ([`Record::to_value_masked`]).
//!
//! ```
//! # use navree_orm::__doc_models::*;
//! use navree_api::ApiResource;
//! use navree_orm::attributes::Resource;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! struct TagResource {
//!     id: i64,
//!     label: String,
//! }
//!
//! impl ApiResource for TagResource {
//!     const TYPE: &'static str = "Tag";
//! }
//!
//! impl Resource<Tag> for TagResource {
//!     fn from_record(tag: &Record<Tag>) -> Self {
//!         TagResource { id: tag.id, label: format!("#{}", tag.name) }
//!     }
//! }
//!
//! let tag = Record::new(Tag { id: 3, name: "vip".into(), color: "gold".into() });
//! assert_eq!(
//!     tag.to_resource_json_as::<TagResource>(),
//!     serde_json::json!({"id": 3, "label": "#vip", "__type": "Tag"})
//! );
//! assert_eq!(tag.to_resource_json()?["__type"], "Tag");
//! # Ok::<(), navree_orm::OrmError>(())
//! ```

use navree_api::{ApiResource, TYPE_FIELD, resource_json};

use crate::attributes::serialize::output_key;
use crate::collection::Collection;
use crate::error::Result;
use crate::model::Model;
use crate::record::Record;

/// A resource built from records of `M`: what an API returns for them
/// (navree-api's [`ApiResource`]: serializable, with a subject type).
pub trait Resource<M: Model>: ApiResource + Sized {
    /// The resource of `record`.
    fn from_record(record: &Record<M>) -> Self;
}

/// The resource a model declares (`#[model(resource = R)]`), so its records
/// turn into it without naming it.
pub trait HasResource: Model {
    /// The model's resource.
    type Resource: Resource<Self>;
}

impl<M: Model> Record<M> {
    /// The record as resource `R`.
    pub fn to_resource<R: Resource<M>>(&self) -> R {
        R::from_record(self)
    }

    /// The record as the resource its model declares
    /// (`#[model(resource = R)]`).
    pub fn resource(&self) -> M::Resource
    where
        M: HasResource,
    {
        M::Resource::from_record(self)
    }

    /// The record as resource `R`, serialized with its `__type`.
    pub fn to_resource_json_as<R: Resource<M>>(&self) -> serde_json::Value {
        resource_json(&R::from_record(self))
    }

    /// The record as its own resource: its serialized JSON (visibility,
    /// appends and relations applied) with `"__type"` set to the model's
    /// name.
    pub fn to_resource_json(&self) -> Result<serde_json::Value> {
        let mut json = self.to_value()?;
        if let serde_json::Value::Object(object) = &mut json {
            object.insert(
                TYPE_FIELD.to_owned(),
                serde_json::Value::String(M::meta().name().to_owned()),
            );
        }
        Ok(json)
    }

    /// The record's JSON keeping only the `allowed` names (columns,
    /// fields, or output keys: what an ability's permitted fields list),
    /// plus the key and `__type`; `None` keeps every name. Field-level
    /// access control for serialized records:
    /// `record.to_value_masked(Some(&ability.permitted_fields("read", "Deal", Some(&json), &columns)))`.
    pub fn to_value_masked(&self, allowed: Option<&[String]>) -> Result<serde_json::Value> {
        let json = self.to_resource_json()?;
        Ok(mask::<M>(json, allowed))
    }

    /// The names a mask chooses from: the keys of the record's
    /// serialized JSON.
    pub fn field_names(&self) -> Result<Vec<String>> {
        Ok(crate::attributes::serialize::record_object(self)?
            .into_iter()
            .map(|(key, _)| key)
            .collect())
    }
}

/// Keeps `allowed` (resolved to output keys), the key and `__type`.
fn mask<M: Model>(json: serde_json::Value, allowed: Option<&[String]>) -> serde_json::Value {
    let Some(allowed) = allowed else {
        return json;
    };
    let meta = M::meta();
    let mut keep: Vec<String> = allowed
        .iter()
        .filter_map(|name| output_key(meta, name))
        .collect();
    if let Some(key) = output_key(meta, meta.key_column()) {
        keep.push(key);
    }
    keep.push(TYPE_FIELD.to_owned());
    match json {
        serde_json::Value::Object(mut object) => {
            object.retain(|key, _| keep.contains(key));
            serde_json::Value::Object(object)
        }
        other => other,
    }
}

impl<M: Model> Collection<M> {
    /// Every record as resource `R`.
    pub fn to_resource_collection<R: Resource<M>>(&self) -> Vec<R> {
        self.iter().map(R::from_record).collect()
    }

    /// Every record as the resource its model declares.
    pub fn resources(&self) -> Vec<M::Resource>
    where
        M: HasResource,
    {
        self.iter().map(M::Resource::from_record).collect()
    }

    /// Every record as resource `R`, serialized with its `__type`, as a
    /// JSON array.
    pub fn to_resource_json_as<R: Resource<M>>(&self) -> serde_json::Value {
        serde_json::Value::Array(self.iter().map(Record::to_resource_json_as::<R>).collect())
    }

    /// Every record's JSON keeping only the `allowed` names (see
    /// [`Record::to_value_masked`]), as a JSON array.
    pub fn to_value_masked(&self, allowed: Option<&[String]>) -> Result<serde_json::Value> {
        self.iter()
            .map(|record| record.to_value_masked(allowed))
            .collect::<Result<Vec<_>>>()
            .map(serde_json::Value::Array)
    }
}
