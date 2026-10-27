//! [`AnyRecord`]: a record of any model, what a `morph_to` relation loads.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::fmt;

use navree_kernel::BoxFuture;
use serde::{Serialize, Serializer};

use crate::error::Result;
use crate::exec::Target;
use crate::model::{Model, PrimaryKey};
use crate::record::Record;
use crate::value::{Field, Value};

/// A [`Record`] with its model erased.
trait ErasedRecord: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn clone_box(&self) -> Box<dyn ErasedRecord>;
    fn to_json(&self) -> std::result::Result<serde_json::Value, serde_json::Error>;
    fn model_type(&self) -> TypeId;
    fn model_name(&self) -> &'static str;
    fn morph_name(&self) -> Cow<'static, str>;
    fn key_value(&self) -> Value;
    fn key_text(&self) -> String;
    fn column(&self, column: &str) -> Result<Option<Value>>;
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>>;
    fn touch_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>>;
}

impl<M: Model> ErasedRecord for Record<M> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn clone_box(&self) -> Box<dyn ErasedRecord> {
        Box::new(self.clone())
    }

    fn to_json(&self) -> std::result::Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn model_type(&self) -> TypeId {
        TypeId::of::<M>()
    }

    fn model_name(&self) -> &'static str {
        M::meta().name()
    }

    fn morph_name(&self) -> Cow<'static, str> {
        M::morph_name()
    }

    fn key_value(&self) -> Value {
        self.key().to_value()
    }

    fn key_text(&self) -> String {
        self.key().to_key_string()
    }

    fn column(&self, column: &str) -> Result<Option<Value>> {
        self.model.value(column)
    }

    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        crate::relations::lazy::push_record(self, target)
    }

    fn touch_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            if crate::relations::touch::is_ignoring_touch_of::<M>() {
                return Ok(false);
            }
            crate::persist::touch(target, self, None).await
        })
    }
}

/// A record of any model: what a `morph_to` relation loads (a comment's
/// `commentable` is a deal or a contact). Downcast it to the record of the
/// model it is, or convert it into an enum of the possible models with
/// [`morph_types!`](crate::morph_types).
///
/// ```
/// # use navree_orm::__doc_models::*;
/// use navree_orm::relations::AnyRecord;
///
/// let any = AnyRecord::new(Record::new(Company { name: "Acme".into(), ..Company::default() }));
/// assert_eq!(any.model_name(), "Company");
/// assert_eq!(any.morph_name(), "company");
/// assert!(any.is::<Company>());
/// assert_eq!(any.downcast_ref::<Company>().unwrap().name, "Acme");
/// assert!(any.downcast_ref::<Deal>().is_none());
/// let company: Record<Company> = any.downcast().ok().unwrap();
/// # let _ = company;
/// ```
pub struct AnyRecord {
    inner: Box<dyn ErasedRecord>,
}

impl AnyRecord {
    /// `record`, with its model erased.
    pub fn new<M: Model>(record: Record<M>) -> Self {
        AnyRecord {
            inner: Box::new(record),
        }
    }

    /// Whether the record is a `Record<M>`.
    pub fn is<M: Model>(&self) -> bool {
        self.inner.model_type() == TypeId::of::<M>()
    }

    /// The record, when it is a `Record<M>`.
    pub fn downcast_ref<M: Model>(&self) -> Option<&Record<M>> {
        self.inner.as_any().downcast_ref::<Record<M>>()
    }

    /// The record for changes, when it is a `Record<M>`.
    pub fn downcast_mut<M: Model>(&mut self) -> Option<&mut Record<M>> {
        self.inner.as_any_mut().downcast_mut::<Record<M>>()
    }

    /// The record, when it is a `Record<M>`; else `self` back.
    pub fn downcast<M: Model>(self) -> std::result::Result<Record<M>, AnyRecord> {
        if !self.is::<M>() {
            return Err(self);
        }
        match self.inner.into_any().downcast::<Record<M>>() {
            Ok(record) => Ok(*record),
            Err(_) => unreachable!("the model type was checked"),
        }
    }

    /// The model's name, e.g. `Deal`.
    pub fn model_name(&self) -> &'static str {
        self.inner.model_name()
    }

    /// The model's morph name (what type columns store), e.g. `deal`.
    pub fn morph_name(&self) -> Cow<'static, str> {
        self.inner.morph_name()
    }

    /// The record's key as a column value.
    pub fn key(&self) -> Value {
        self.inner.key_value()
    }

    /// The record's key as text.
    pub fn key_string(&self) -> String {
        self.inner.key_text()
    }

    /// The stored value of `column`; `None` when the model has no such
    /// column.
    pub fn value(&self, column: &str) -> Result<Option<Value>> {
        self.inner.column(column)
    }

    /// Saves the record and its loaded relations (`Record::push`).
    pub(crate) fn push_on<'a, 'b>(
        &'a mut self,
        target: &'a Target<'b>,
    ) -> BoxFuture<'a, Result<bool>> {
        self.inner.push_on(target)
    }

    /// Sets the record's update timestamp to now and saves it (unless a
    /// scope ignores touching its model).
    pub(crate) fn touch_on<'a, 'b>(
        &'a mut self,
        target: &'a Target<'b>,
    ) -> BoxFuture<'a, Result<bool>> {
        self.inner.touch_on(target)
    }
}

impl<M: Model> From<Record<M>> for AnyRecord {
    fn from(record: Record<M>) -> Self {
        AnyRecord::new(record)
    }
}

impl Clone for AnyRecord {
    fn clone(&self) -> Self {
        AnyRecord {
            inner: self.inner.clone_box(),
        }
    }
}

impl fmt::Debug for AnyRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyRecord")
            .field("model", &self.model_name())
            .field("key", &self.key_string())
            .finish_non_exhaustive()
    }
}

/// The record, serialized as its model would be.
impl Serialize for AnyRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.inner
            .to_json()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// Declares an enum of the models a `morph_to` relation can load, with a
/// conversion from [`AnyRecord`] and a function registering every model in
/// the [`MorphMap`](crate::relations::MorphMap).
///
/// ```
/// # use navree_orm::__doc_models::*;
/// navree_orm::morph_types! {
///     /// What an activity is about.
///     pub enum Subject {
///         Deal(Deal),
///         Company(Company),
///     }
/// }
///
/// Subject::register();
/// let any = navree_orm::relations::AnyRecord::new(Record::new(Company::default()));
/// match Subject::try_from(any).unwrap() {
///     Subject::Company(company) => assert_eq!(company.name, ""),
///     Subject::Deal(_) => unreachable!(),
/// }
/// ```
#[macro_export]
macro_rules! morph_types {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident($model:ty)),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone)]
        $vis enum $name {
            $(
                #[doc = concat!("A `", stringify!($model), "`.")]
                $variant($crate::Record<$model>),
            )+
        }

        impl $name {
            /// Registers every model of the enum in the morph map.
            $vis fn register() {
                $($crate::relations::MorphMap::register::<$model>();)+
            }

            /// The record, with its model erased.
            $vis fn into_any(self) -> $crate::relations::AnyRecord {
                match self {
                    $($name::$variant(record) => $crate::relations::AnyRecord::new(record),)+
                }
            }
        }

        impl ::core::convert::TryFrom<$crate::relations::AnyRecord> for $name {
            type Error = $crate::relations::AnyRecord;

            fn try_from(
                record: $crate::relations::AnyRecord,
            ) -> ::core::result::Result<Self, $crate::relations::AnyRecord> {
                $(
                    let record = match record.downcast::<$model>() {
                        ::core::result::Result::Ok(record) => {
                            return ::core::result::Result::Ok($name::$variant(record));
                        }
                        ::core::result::Result::Err(record) => record,
                    };
                )+
                ::core::result::Result::Err(record)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                match self {
                    $($name::$variant(record) => ::serde::Serialize::serialize(record, serializer),)+
                }
            }
        }
    };
}
