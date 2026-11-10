//! [`FactoryContext`]: what a definition and its states see while one
//! record is built, and [`Attributes`]: a record's generated values.

use std::borrow::Cow;
use std::marker::PhantomData;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use crate::column::{Column, ColumnName};
use crate::factory::faker::Faker;
use crate::factory::sources::{ParentSource, ResolveParent};
use crate::model::Model;
use crate::value::{Field, Value, ValueFor, to_json};

/// What a factory's definition and states see while one record is built:
/// its position in the batch, fake data, the current time, and the parents
/// it declares.
///
/// ```
/// use navree_orm::factory::{FactoryContext, ModelFactory};
/// use navree_orm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// #[model(table = "people")]
/// pub struct Person {
///     id: i64,
///     name: String,
///     email: String,
///     rank: i64,
/// }
///
/// #[derive(Default)]
/// pub struct PersonFactory;
///
/// impl ModelFactory for PersonFactory {
///     type Model = Person;
///
///     fn definition(&self, ctx: &mut FactoryContext<'_, Person>) -> Person {
///         Person {
///             name: ctx.fake().name(),
///             email: ctx.fake().unique(|f| f.safe_email()),
///             rank: ctx.index() as i64 + 1,
///             ..Person::default()
///         }
///     }
/// }
/// ```
pub struct FactoryContext<'a, M: Model> {
    index: usize,
    count: usize,
    now: DateTime<Utc>,
    faker: &'a mut Faker,
    parents: &'a mut Vec<PendingParent>,
    _model: PhantomData<fn() -> M>,
}

impl<'a, M: Model> FactoryContext<'a, M> {
    pub(crate) fn new(
        index: usize,
        count: usize,
        now: DateTime<Utc>,
        faker: &'a mut Faker,
        parents: &'a mut Vec<PendingParent>,
    ) -> Self {
        FactoryContext {
            index,
            count,
            now,
            faker,
            parents,
            _model: PhantomData,
        }
    }

    /// The record's position in the batch, from 0 (each `make` or `create`
    /// call starts again at 0).
    pub fn index(&self) -> usize {
        self.index
    }

    /// How many records the batch builds.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Fake data for the record (seeded with [`Factory::seed`](crate::factory::Factory::seed)).
    pub fn fake(&mut self) -> &mut Faker {
        self.faker
    }

    /// The current time: the app's clock when the records are created
    /// through an executor of an app (tests freeze it with a fixed clock),
    /// else the system clock; whole microseconds.
    pub fn now(&self) -> DateTime<Utc> {
        self.now
    }

    /// Declares that the record belongs to a parent from `parent` through
    /// `column`, and returns the column's empty value for the definition to
    /// use in the meantime.
    ///
    /// When the record is created and `column` still holds that empty
    /// value (no state, `set`, `for_` or sequence gave it one), the column
    /// gets the parent's key: a recycled record of the parent's model when
    /// the factory recycles some ([`Factory::recycle`](crate::factory::Factory::recycle)),
    /// else a parent created from `parent` for this record (one per
    /// record), unless parents are off
    /// ([`Factory::without_parents`](crate::factory::Factory::without_parents)).
    /// `make` never writes: it uses a recycled record or a stored one, or
    /// leaves the column empty.
    ///
    /// ```
    /// # use navree_orm::factory::{FactoryContext, ModelFactory};
    /// # use navree_orm::prelude::*;
    /// # #[derive(Model, Clone, Debug, Default, serde::Serialize)]
    /// # #[model(factory = CompanyFactory)]
    /// # pub struct Company { id: i64, name: String }
    /// # #[derive(Default)]
    /// # pub struct CompanyFactory;
    /// # impl ModelFactory for CompanyFactory {
    /// #     type Model = Company;
    /// #     fn definition(&self, ctx: &mut FactoryContext<'_, Company>) -> Company {
    /// #         Company { name: ctx.fake().company(), ..Company::default() }
    /// #     }
    /// # }
    /// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
    /// pub struct Contact {
    ///     id: i64,
    ///     company_id: i64,
    ///     name: String,
    /// }
    ///
    /// #[derive(Default)]
    /// pub struct ContactFactory;
    ///
    /// impl ModelFactory for ContactFactory {
    ///     type Model = Contact;
    ///
    ///     fn definition(&self, ctx: &mut FactoryContext<'_, Contact>) -> Contact {
    ///         Contact {
    ///             company_id: ctx.belongs_to(Contact::COMPANY_ID, Company::factory()),
    ///             name: ctx.fake().name(),
    ///             ..Contact::default()
    ///         }
    ///     }
    /// }
    /// ```
    pub fn belongs_to<T, S>(&mut self, column: Column<M, T>, parent: S) -> T
    where
        T: Field + Default,
        S: ParentSource,
        <S::Parent as Model>::Key: ValueFor<T>,
    {
        let empty = T::default();
        let placeholder = column
            .encode(&empty)
            .unwrap_or_else(|_| <T as Field>::null());
        let name: Cow<'static, str> = Cow::Owned(column.column_name().into_owned());
        self.parents.retain(|pending| pending.column != name);
        self.parents.push(PendingParent {
            column: name,
            placeholder,
            source: parent.into_parent().0,
        });
        empty
    }
}

/// A parent declared by a definition with [`FactoryContext::belongs_to`],
/// resolved once the states have run.
pub(crate) struct PendingParent {
    pub(crate) column: Cow<'static, str>,
    pub(crate) placeholder: Value,
    pub(crate) source: Arc<dyn ResolveParent>,
}

impl PendingParent {
    /// Whether `model`'s column still holds the placeholder (nothing else
    /// set it).
    pub(crate) fn is_open<M: Model>(&self, model: &M) -> crate::Result<bool> {
        Ok(match model.value(&self.column)? {
            Some(value) => crate::value::same_value(&value, &self.placeholder),
            None => false,
        })
    }
}

/// The values a factory generated for one record, by column (see
/// [`Factory::raw`](crate::factory::Factory::raw)): the definition and the
/// states, without an empty key or empty timestamps. Serializes as a JSON
/// object, so a test can post it as a request body.
///
/// ```
/// use navree_orm::Value;
/// use navree_orm::factory::Attributes;
///
/// let attributes = Attributes::from(vec![("title", Value::from("Renewal")), ("amount_cents", Value::from(1200_i64))]);
/// assert_eq!(attributes.get("title"), Some(&Value::from("Renewal")));
/// assert_eq!(attributes.to_json(), serde_json::json!({"title": "Renewal", "amount_cents": 1200}));
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attributes {
    values: Vec<(&'static str, Value)>,
}

impl Attributes {
    /// The value of `column`, if it was generated.
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(name, _)| *name == column)
            .map(|(_, value)| value)
    }

    /// The columns and values, in field order.
    pub fn iter(&self) -> impl Iterator<Item = &(&'static str, Value)> {
        self.values.iter()
    }

    /// How many columns there are.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether there are no columns.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The columns and values.
    pub fn into_vec(self) -> Vec<(&'static str, Value)> {
        self.values
    }

    /// The values as a JSON object, by column.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Object(
            self.values
                .iter()
                .map(|(column, value)| ((*column).to_owned(), to_json(value)))
                .collect(),
        )
    }
}

impl From<Vec<(&'static str, Value)>> for Attributes {
    fn from(values: Vec<(&'static str, Value)>) -> Self {
        Attributes { values }
    }
}

impl IntoIterator for Attributes {
    type Item = (&'static str, Value);
    type IntoIter = std::vec::IntoIter<(&'static str, Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

/// A JSON object of the values, by column.
impl Serialize for Attributes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}
