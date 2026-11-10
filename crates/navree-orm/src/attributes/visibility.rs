//! What a serialized record shows, per record and per collection: hidden
//! and visible names, and appended accessors.
//!
//! A record starts with its model's lists (`#[model(hidden(..),
//! visible(..), appends(..))]`) and can change them for itself:
//!
//! ```
//! # use navree_orm::__doc_models::*;
//! let mut tag = Record::new(Tag { id: 1, name: "vip".into(), color: "gold".into() });
//! tag.make_hidden(["color"]);
//! assert_eq!(tag.to_value()?, serde_json::json!({"id": 1, "name": "vip"}));
//! tag.make_visible(["color"]).set_visible(["name"]);
//! assert_eq!(tag.to_value()?, serde_json::json!({"name": "vip"}));
//! # Ok::<(), navree_orm::OrmError>(())
//! ```

use std::borrow::Cow;

use crate::attributes::state::Names;
use crate::collection::Collection;
use crate::error::{OrmError, Result};
use crate::model::Model;
use crate::record::Record;

/// A condition for [`Record::make_visible_if`] and
/// [`Record::make_hidden_if`]: a `bool`, or a function of the record.
pub trait Condition<T> {
    /// Whether the condition holds for `subject`.
    fn holds(self, subject: &T) -> bool;
}

impl<T> Condition<T> for bool {
    fn holds(self, _subject: &T) -> bool {
        self
    }
}

impl<T, F: FnOnce(&T) -> bool> Condition<T> for F {
    fn holds(self, subject: &T) -> bool {
        self(subject)
    }
}

/// Names from anything that turns into them.
fn names<N: Into<Cow<'static, str>>>(names: impl IntoIterator<Item = N>) -> Names {
    names.into_iter().map(Into::into).collect()
}

/// `list` followed by the names of `more` it does not have yet.
fn merged(mut list: Names, more: Names) -> Names {
    for name in more {
        if !list.contains(&name) {
            list.push(name);
        }
    }
    list
}

fn defaults(names: &'static [&'static str]) -> Names {
    names.iter().map(|name| Cow::Borrowed(*name)).collect()
}

impl<M: Model> Record<M> {
    /// The names serialized records of this record leave out.
    pub fn hidden(&self) -> Vec<&str> {
        match &self.attributes.hidden {
            Some(list) => list.iter().map(AsRef::as_ref).collect(),
            None => M::meta().hidden_names().to_vec(),
        }
    }

    /// The only names serialized records of this record keep; empty keeps
    /// every name.
    pub fn visible(&self) -> Vec<&str> {
        match &self.attributes.visible {
            Some(list) => list.iter().map(AsRef::as_ref).collect(),
            None => M::meta().visible_names().to_vec(),
        }
    }

    /// The accessors serialized records of this record append.
    pub fn appends(&self) -> Vec<&str> {
        self.appended_names()
    }

    pub(crate) fn hidden_names(&self) -> Names {
        self.attributes
            .hidden
            .clone()
            .unwrap_or_else(|| defaults(M::meta().hidden_names()))
    }

    pub(crate) fn visible_names(&self) -> Names {
        self.attributes
            .visible
            .clone()
            .unwrap_or_else(|| defaults(M::meta().visible_names()))
    }

    pub(crate) fn appended_names(&self) -> Vec<&str> {
        match &self.attributes.appends {
            Some(list) => list.iter().map(AsRef::as_ref).collect(),
            None => M::meta().appended_names().to_vec(),
        }
    }

    /// Shows `names` again: they leave the hidden list, and join the visible
    /// list when there is one.
    pub fn make_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let shown = self::names(names);
        let hidden: Names = self
            .hidden_names()
            .into_iter()
            .filter(|name| !shown.contains(name))
            .collect();
        self.attributes.hidden = Some(hidden);
        let visible = self.visible_names();
        if !visible.is_empty() {
            self.attributes.visible = Some(merged(visible, shown));
        }
        self
    }

    /// [`make_visible`](Record::make_visible) when `condition` holds (a
    /// `bool`, or a function of the record).
    pub fn make_visible_if<N: Into<Cow<'static, str>>>(
        &mut self,
        condition: impl Condition<Self>,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        if condition.holds(self) {
            self.make_visible(names);
        }
        self
    }

    /// Hides `names` too.
    pub fn make_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let hidden = merged(self.hidden_names(), self::names(names));
        self.attributes.hidden = Some(hidden);
        self
    }

    /// [`make_hidden`](Record::make_hidden) when `condition` holds.
    pub fn make_hidden_if<N: Into<Cow<'static, str>>>(
        &mut self,
        condition: impl Condition<Self>,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        if condition.holds(self) {
            self.make_hidden(names);
        }
        self
    }

    /// Replaces the hidden names.
    pub fn set_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        self.attributes.hidden = Some(self::names(names));
        self
    }

    /// Adds to the hidden names.
    pub fn merge_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        self.make_hidden(names)
    }

    /// Replaces the visible names (empty keeps every name).
    pub fn set_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        self.attributes.visible = Some(self::names(names));
        self
    }

    /// Adds to the visible names.
    pub fn merge_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let visible = merged(self.visible_names(), self::names(names));
        self.attributes.visible = Some(visible);
        self
    }

    /// Appends the accessors `names` to serialized records of this record;
    /// a name that is not an accessor of the model is an error.
    pub fn append<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<&mut Self> {
        let appends = merged(
            self.current_appends(),
            checked_accessors::<M>(self::names(names))?,
        );
        self.attributes.appends = Some(appends);
        Ok(self)
    }

    /// Replaces the appended accessors.
    pub fn set_appends<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<&mut Self> {
        self.attributes.appends = Some(checked_accessors::<M>(self::names(names))?);
        Ok(self)
    }

    /// Adds to the appended accessors (like [`append`](Record::append)).
    pub fn merge_appends<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<&mut Self> {
        self.append(names)
    }

    /// Appends no accessor.
    pub fn without_appends(&mut self) -> &mut Self {
        self.attributes.appends = Some(Vec::new());
        self
    }

    /// Whether serialized records of this record append accessor `name`.
    pub fn has_appended(&self, name: &str) -> bool {
        self.appended_names().contains(&name)
    }

    fn current_appends(&self) -> Names {
        self.attributes
            .appends
            .clone()
            .unwrap_or_else(|| defaults(M::meta().appended_names()))
    }
}

/// `names`, each an accessor of `M`.
fn checked_accessors<M: Model>(names: Names) -> Result<Names> {
    let accessors = M::accessors();
    for name in &names {
        if !accessors
            .iter()
            .any(|accessor| accessor.name() == name.as_ref())
        {
            let known: Vec<&str> = accessors.iter().map(|accessor| accessor.name()).collect();
            return Err(OrmError::InvalidQuery(format!(
                "{} has no accessor named `{name}` to append ({})",
                M::meta().name(),
                if known.is_empty() {
                    "it has no #[accessors] block".to_owned()
                } else {
                    format!("its accessors: {}", known.join(", "))
                }
            )));
        }
    }
    Ok(names)
}

impl<M: Model> Collection<M> {
    /// Hides `names` in every record.
    pub fn make_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let names = self::names(names);
        for record in self.iter_mut() {
            record.make_hidden(names.iter().cloned());
        }
        self
    }

    /// Adds to the hidden names of every record.
    pub fn merge_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        self.make_hidden(names)
    }

    /// Replaces the hidden names of every record.
    pub fn set_hidden<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let names = self::names(names);
        for record in self.iter_mut() {
            record.set_hidden(names.iter().cloned());
        }
        self
    }

    /// Shows `names` again in every record.
    pub fn make_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let names = self::names(names);
        for record in self.iter_mut() {
            record.make_visible(names.iter().cloned());
        }
        self
    }

    /// Adds to the visible names of every record.
    pub fn merge_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let names = self::names(names);
        for record in self.iter_mut() {
            record.merge_visible(names.iter().cloned());
        }
        self
    }

    /// Replaces the visible names of every record.
    pub fn set_visible<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> &mut Self {
        let names = self::names(names);
        for record in self.iter_mut() {
            record.set_visible(names.iter().cloned());
        }
        self
    }

    /// Appends the accessors `names` to every record.
    pub fn append<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<&mut Self> {
        let names = checked_accessors::<M>(self::names(names))?;
        for record in self.iter_mut() {
            record.append(names.iter().cloned())?;
        }
        Ok(self)
    }

    /// Replaces the appended accessors of every record.
    pub fn set_appends<N: Into<Cow<'static, str>>>(
        &mut self,
        names: impl IntoIterator<Item = N>,
    ) -> Result<&mut Self> {
        let names = checked_accessors::<M>(self::names(names))?;
        for record in self.iter_mut() {
            record.set_appends(names.iter().cloned())?;
        }
        Ok(self)
    }

    /// Appends no accessor in any record.
    pub fn without_appends(&mut self) -> &mut Self {
        for record in self.iter_mut() {
            record.without_appends();
        }
        self
    }
}
