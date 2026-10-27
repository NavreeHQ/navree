//! Where a column is: [`Scope`] resolves column references to document
//! fields or to `$lookup` variables, and [`Cx`] names temporary fields and
//! variables.

use std::cell::RefCell;

use bson::Bson;
use navree_orm::OrmError;
use navree_orm::ir::{ColumnRef, JsonSegment, TableRef};

use crate::error::invalid;

/// The names one compile makes up: temporary fields (`__navree_in0`) and
/// `$lookup` variables (`navree1`), unique within the command.
#[derive(Debug, Default)]
pub(crate) struct Cx {
    next: usize,
}

impl Cx {
    /// A temporary field: `__navree_<kind><n>`.
    pub(crate) fn field(&mut self, kind: &str) -> String {
        let n = self.next;
        self.next += 1;
        format!("__navree_{kind}{n}")
    }

    /// A `$lookup` variable (MongoDB wants them to start with a lowercase
    /// letter): `navree<n>`.
    fn variable(&mut self) -> String {
        let n = self.next;
        self.next += 1;
        format!("navree{n}")
    }
}

/// Where a column's value is, seen from a stage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Place {
    /// A field (dot path) of the document the stage sees.
    Field(String),
    /// A variable a `$lookup` bound from the enclosing pipeline's document.
    Variable(String),
}

impl Place {
    /// As an aggregation expression: `"$path"` or `"$$variable"`.
    pub(crate) fn expr(&self) -> Bson {
        match self {
            Place::Field(path) => Bson::String(format!("${path}")),
            Place::Variable(name) => Bson::String(format!("$${name}")),
        }
    }
}

/// The documents a pipeline sees and how columns reach them: the
/// collection's own fields (qualified by its name or alias, or not at
/// all), joined documents under their alias, and the columns of the query
/// this one is a subquery of, bound as `$lookup` variables.
pub(crate) struct Scope<'p> {
    /// What the collection's columns are qualified by: its name and alias.
    own: Vec<String>,
    /// Joined documents: (qualifier, the field they live under).
    joined: Vec<(String, String)>,
    /// The query this one is a subquery (or a join's pipeline) of.
    parent: Option<&'p Scope<'p>>,
    /// The variables bound from the parent: (name, expression in the
    /// parent's document).
    lets: RefCell<Vec<(String, Bson)>>,
}

impl<'p> Scope<'p> {
    /// The scope of a query of `table`.
    pub(crate) fn of(table: &TableRef) -> Scope<'static> {
        Scope {
            own: qualifiers(table),
            joined: Vec::new(),
            parent: None,
            lets: RefCell::new(Vec::new()),
        }
    }

    /// The scope of a subquery of `table` inside this one.
    pub(crate) fn child<'a>(&'a self, table: &TableRef) -> Scope<'a> {
        Scope {
            own: qualifiers(table),
            joined: Vec::new(),
            parent: Some(self),
            lets: RefCell::new(Vec::new()),
        }
    }

    /// The scope of a derived table (`FROM (subquery) AS alias`) of this
    /// query: its stages run before this query's, at the same level, so it
    /// reaches the same enclosing query.
    pub(crate) fn sibling(&self, table: &TableRef) -> Scope<'p> {
        Scope {
            own: qualifiers(table),
            joined: Vec::new(),
            parent: self.parent,
            lets: RefCell::new(Vec::new()),
        }
    }

    /// Takes over the variables a [`sibling`](Self::sibling) bound (they
    /// are bound from the same enclosing query).
    pub(crate) fn absorb(&self, sibling: &Scope<'_>) {
        let mut lets = self.lets.borrow_mut();
        for binding in sibling.lets.borrow().iter() {
            if !lets.contains(binding) {
                lets.push(binding.clone());
            }
        }
    }

    /// Makes a joined table's columns (qualified by `qualifier`) reach the
    /// document under `field`.
    pub(crate) fn join(&mut self, qualifier: &str, field: &str) {
        self.joined.push((qualifier.to_owned(), field.to_owned()));
    }

    /// The fields joined documents live under.
    pub(crate) fn joined_fields(&self) -> impl Iterator<Item = &str> {
        self.joined.iter().map(|(_, field)| field.as_str())
    }

    /// The collection's name (the first qualifier).
    pub(crate) fn name(&self) -> &str {
        self.own.first().map_or("", String::as_str)
    }

    /// Whether `qualifier` names the collection itself (its name or alias).
    pub(crate) fn is_own(&self, qualifier: &str) -> bool {
        self.own.iter().any(|own| own == qualifier)
    }

    /// Where `column` is.
    pub(crate) fn place(&self, cx: &mut Cx, column: &ColumnRef) -> Result<Place, OrmError> {
        let path = field_path(column)?;
        let Some(qualifier) = column.table.as_deref() else {
            return Ok(Place::Field(path));
        };
        if self.own.iter().any(|own| own == qualifier) {
            return Ok(Place::Field(path));
        }
        if let Some((_, field)) = self.joined.iter().find(|(name, _)| name == qualifier) {
            return Ok(Place::Field(format!("{field}.{path}")));
        }
        match self.parent {
            Some(parent) => {
                let outer = parent.place(cx, column)?;
                Ok(Place::Variable(self.bind(cx, outer.expr())))
            }
            None => Err(invalid(format!(
                "the column `{column}` is qualified by `{qualifier}`, which is neither the collection `{}` nor a joined one (a field of an embedded document is `{qualifier}->{}`)",
                self.name(),
                column.name
            ))),
        }
    }

    /// The variable bound to `expr` (in the parent's document), made on
    /// first use.
    fn bind(&self, cx: &mut Cx, expr: Bson) -> String {
        let mut lets = self.lets.borrow_mut();
        if let Some((name, _)) = lets.iter().find(|(_, bound)| *bound == expr) {
            return name.clone();
        }
        let name = cx.variable();
        lets.push((name.clone(), expr));
        name
    }

    /// The variables bound from the parent so far (a `$lookup`'s `let`).
    pub(crate) fn lets(&self) -> Vec<(String, Bson)> {
        self.lets.borrow().clone()
    }
}

/// What a table's columns are qualified by: its name and its alias.
fn qualifiers(table: &TableRef) -> Vec<String> {
    let mut names = vec![table.name.to_string()];
    if let Some(alias) = &table.alias
        && alias != &table.name
    {
        names.push(alias.to_string());
    }
    names
}

/// The dot path of `column` (without its table): the field, then the keys
/// and indexes of its JSON path.
pub(crate) fn field_path(column: &ColumnRef) -> Result<String, OrmError> {
    let mut path = field_name(&column.name, column)?.to_owned();
    for segment in &column.path {
        match segment {
            JsonSegment::Key(key) => {
                path.push('.');
                path.push_str(field_name(key, column)?);
            }
            JsonSegment::Index(index) if *index >= 0 => {
                path.push('.');
                path.push_str(&index.to_string());
            }
            JsonSegment::Index(index) => {
                return Err(invalid(format!(
                    "the path of `{column}` has the index {index}: MongoDB paths count array elements from the start"
                )));
            }
        }
    }
    Ok(path)
}

/// `name` checked as one step of a field path.
fn field_name<'a>(name: &'a str, column: &ColumnRef) -> Result<&'a str, OrmError> {
    if name.is_empty() {
        return Err(invalid(format!("`{column}` has an empty field name")));
    }
    if name.starts_with('$') || name.contains('.') || name.contains('\0') {
        return Err(invalid(format!(
            "`{name}` (in `{column}`) is not a MongoDB field name: names cannot start with `$` or contain `.`; write paths into embedded documents as `field->key`"
        )));
    }
    Ok(name)
}

/// `name` checked as the name of an output field (an alias).
pub(crate) fn output_name(name: &str) -> Result<&str, OrmError> {
    if name.is_empty() || name.starts_with('$') || name.contains('.') || name.contains('\0') {
        return Err(invalid(format!(
            "`{name}` cannot name a MongoDB field (names cannot be empty, start with `$` or contain `.`): pick another alias"
        )));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_resolve_by_qualifier() {
        let mut cx = Cx::default();
        let mut scope = Scope::of(&TableRef::aliased("deals", "d"));
        scope.join("companies", "companies");
        let place = |scope: &Scope<'_>, cx: &mut Cx, text: &'static str| {
            scope.place(cx, &ColumnRef::parse(text))
        };
        assert_eq!(place(&scope, &mut cx, "title").unwrap(), Place::Field("title".into()));
        assert_eq!(place(&scope, &mut cx, "d.title").unwrap(), Place::Field("title".into()));
        assert_eq!(
            place(&scope, &mut cx, "deals.meta->tags->0").unwrap(),
            Place::Field("meta.tags.0".into())
        );
        assert_eq!(
            place(&scope, &mut cx, "companies.name").unwrap(),
            Place::Field("companies.name".into())
        );
        let error = place(&scope, &mut cx, "address.city").unwrap_err();
        assert!(error.to_string().contains("`address->city`"), "{error}");
        assert!(place(&scope, &mut cx, "meta->tags[-1]").is_err());
        assert!(place(&scope, &mut cx, "$where").is_err());
    }

    #[test]
    fn subqueries_bind_outer_columns_as_variables() {
        let mut cx = Cx::default();
        let outer = Scope::of(&TableRef::new("companies"));
        let inner = outer.child(&TableRef::new("deals"));
        let company = ColumnRef::parse("companies.id");
        assert_eq!(
            inner.place(&mut cx, &company).unwrap(),
            Place::Variable("navree0".into())
        );
        // The same column binds once.
        assert_eq!(
            inner.place(&mut cx, &company).unwrap(),
            Place::Variable("navree0".into())
        );
        assert_eq!(
            inner.place(&mut cx, &ColumnRef::parse("deals.company_id")).unwrap(),
            Place::Field("company_id".into())
        );
        assert_eq!(inner.lets(), [("navree0".to_owned(), Bson::String("$id".into()))]);
        // Two levels down, the binding goes through the middle pipeline.
        let innermost = inner.child(&TableRef::new("activities"));
        assert_eq!(
            innermost.place(&mut cx, &company).unwrap(),
            Place::Variable("navree1".into())
        );
        assert_eq!(
            innermost.lets(),
            [("navree1".to_owned(), Bson::String("$$navree0".into()))]
        );
    }
}
