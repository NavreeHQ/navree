//! The registered exports, by name, with their types erased.

use std::collections::BTreeMap;
use std::sync::Arc;

use http::Uri;
use navree_contracts::BoxError;
use navree_http::Query;
use navree_kernel::{AppContext, BoxFuture, Injectable, ModuleId};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Export, ExportContext, ExportPage};
use crate::{Cell, Column, Exportable, SheetsError};

/// Opens a registered export with its parameters.
type OpenFn =
    fn(AppContext, ModuleId, Value) -> BoxFuture<'static, Result<Arc<dyn Source>, SheetsError>>;

/// An export registered by a module (a contribution of `m.export::<E>()`).
#[derive(Clone)]
pub(crate) struct ExportRegistration {
    pub(crate) name: &'static str,
    pub(crate) module: ModuleId,
    pub(crate) permission: Option<String>,
    pub(crate) columns: fn() -> Vec<Column>,
    pub(crate) sheet: fn() -> &'static str,
    /// The parameters from a query string, as JSON.
    pub(crate) query_params: fn(&Uri) -> Result<Value, SheetsError>,
    /// The parameters from a JSON body (`None`: not given), checked.
    pub(crate) json_params: fn(Option<Value>) -> Result<Value, SheetsError>,
    open: OpenFn,
}

impl ExportRegistration {
    pub(crate) fn of<E: Export + Injectable>(module: ModuleId) -> Self {
        ExportRegistration {
            name: E::NAME,
            module,
            permission: E::permission(),
            columns: <E::Row as Exportable>::columns,
            sheet: <E::Row as Exportable>::sheet_name,
            query_params: query_params::<E>,
            json_params: json_params::<E>,
            open: open::<E>,
        }
    }

    /// The export with `params` (JSON from [`query_params`] or
    /// [`json_params`]).
    pub(crate) async fn open(
        &self,
        app: AppContext,
        params: Value,
    ) -> Result<Arc<dyn Source>, SheetsError> {
        (self.open)(app, self.module, params).await
    }
}

/// An export opened with its parameters: what writing it needs.
pub(crate) trait Source: Send + Sync {
    /// The file name without the extension.
    fn filename(&self) -> String;

    /// How many rows it has, if known.
    fn count<'a>(&'a self, ctx: &'a ExportContext) -> BoxFuture<'a, Result<Option<u64>, BoxError>>;

    /// One page of cells.
    fn page<'a>(
        &'a self,
        ctx: &'a ExportContext,
        page: ExportPage,
    ) -> BoxFuture<'a, Result<Vec<Vec<Cell>>, BoxError>>;
}

struct Typed<E: Export> {
    export: Arc<E>,
    params: E::Params,
}

impl<E: Export> Source for Typed<E> {
    fn filename(&self) -> String {
        self.export.filename(&self.params)
    }

    fn count<'a>(&'a self, ctx: &'a ExportContext) -> BoxFuture<'a, Result<Option<u64>, BoxError>> {
        Box::pin(self.export.count(ctx, &self.params))
    }

    fn page<'a>(
        &'a self,
        ctx: &'a ExportContext,
        page: ExportPage,
    ) -> BoxFuture<'a, Result<Vec<Vec<Cell>>, BoxError>> {
        Box::pin(async move {
            let rows = self.export.rows(ctx, &self.params, page).await?;
            Ok(rows.iter().map(Exportable::cells).collect())
        })
    }
}

fn open<E: Export + Injectable>(
    app: AppContext,
    module: ModuleId,
    params: Value,
) -> BoxFuture<'static, Result<Arc<dyn Source>, SheetsError>> {
    Box::pin(async move {
        let params: E::Params = from_json(params)?;
        let export = app
            .resolver_for(module)
            .get::<E>()
            .await
            .map_err(|error| SheetsError::App(error.into()))?;
        Ok(Arc::new(Typed { export, params }) as Arc<dyn Source>)
    })
}

/// `P` from JSON: `params` itself, else (for `()` and other unit types) from
/// `null` when `params` is an empty object.
pub(crate) fn from_json<P: DeserializeOwned>(params: Value) -> Result<P, SheetsError> {
    match serde_json::from_value::<P>(params.clone()) {
        Ok(parsed) => Ok(parsed),
        Err(error) => {
            let empty = params.is_null() || params.as_object().is_some_and(|o| o.is_empty());
            if empty && let Ok(parsed) = serde_json::from_value::<P>(Value::Null) {
                return Ok(parsed);
            }
            if params.is_null()
                && let Ok(parsed) = serde_json::from_value::<P>(Value::Object(Default::default()))
            {
                return Ok(parsed);
            }
            Err(SheetsError::Invalid(format!(
                "invalid export parameters: {error}"
            )))
        }
    }
}

fn query_params<E: Export>(uri: &Uri) -> Result<Value, SheetsError> {
    let has_params = uri.query().is_some_and(|query| {
        query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .any(|pair| pair.split('=').next() != Some("format"))
    });
    let parsed = match Query::<E::Params>::try_from_uri(uri) {
        Ok(Query(params)) => params,
        Err(error) => {
            if has_params {
                return Err(SheetsError::Invalid(format!(
                    "invalid export parameters: {}",
                    error.body_text()
                )));
            }
            // `()` and other types a query string cannot fill.
            from_json::<E::Params>(Value::Null)?
        }
    };
    Ok(serde_json::to_value(parsed)?)
}

fn json_params<E: Export>(params: Option<Value>) -> Result<Value, SheetsError> {
    let parsed: E::Params = from_json(params.unwrap_or(Value::Null))?;
    Ok(serde_json::to_value(parsed)?)
}

/// The exports of the app, by name.
#[derive(Default)]
pub(crate) struct ExportRegistry {
    by_name: BTreeMap<&'static str, ExportRegistration>,
}

impl ExportRegistry {
    /// Every export the app's modules registered; two with one name are an
    /// error.
    pub(crate) fn build(app: &AppContext) -> Result<Self, String> {
        let mut by_name = BTreeMap::new();
        for contributed in app.contributions::<ExportRegistration>() {
            let registration = (*contributed.value).clone();
            check_name("export", registration.name)?;
            if by_name.insert(registration.name, registration).is_some() {
                return Err(format!(
                    "two exports are named `{}` (m.export::<E>())",
                    contributed.value.name
                ));
            }
        }
        Ok(ExportRegistry { by_name })
    }

    pub(crate) fn get(&self, name: &str) -> Result<&ExportRegistration, SheetsError> {
        self.by_name
            .get(name)
            .ok_or_else(|| SheetsError::UnknownExport(name.to_owned()))
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &ExportRegistration> {
        self.by_name.values()
    }
}

/// Names of exports and importers: letters, digits, `-`, `_` and `.`.
pub(crate) fn check_name(kind: &str, name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!(
            "the {kind} name `{name}` must be letters, digits, `-`, `_` and `.`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
    #[serde(default)]
    struct Filter {
        company: Option<String>,
        limit: u32,
    }

    #[test]
    fn parameters_come_from_json_or_nothing() {
        assert_eq!(from_json::<()>(Value::Null).unwrap(), ());
        assert_eq!(from_json::<()>(serde_json::json!({})).unwrap(), ());
        assert_eq!(from_json::<Filter>(Value::Null).unwrap(), Filter::default());
        assert_eq!(
            from_json::<Filter>(serde_json::json!({ "limit": 5 })).unwrap(),
            Filter {
                company: None,
                limit: 5
            }
        );
        let error = from_json::<Filter>(serde_json::json!({ "limit": "x" })).unwrap_err();
        assert!(
            error.to_string().starts_with("invalid export parameters"),
            "{error}"
        );
    }

    #[test]
    fn names_are_checked() {
        assert!(check_name("export", "contacts.v2-all_x").is_ok());
        assert!(check_name("export", "").is_err());
        assert!(check_name("export", "a/b").is_err());
    }
}
