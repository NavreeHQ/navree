//! [`CurrentZone`] (the request's zone) and [`WithZone`] (a response that
//! names the zone its dates were counted in).

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use chrono::DateTime;
use chrono_tz::Tz;
use http::request::Parts;
use navree_contracts::{AuthState, Clock, ZoneLevel, ZoneRequest};
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, Json};
use navree_kernel::AppContext;
use serde::ser::Error as _;
use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::{LocalDay, Zone, Zones};

/// The zone of the current request: the signed-in user's, else the
/// request's workspace's, else its organization's, else the app's default
/// zone (`time.default_zone`). See [`Zones`] for how each level resolves.
///
/// As an extractor it resolves once per request (later extractions reuse
/// it); it answers 500 when `TimeModule` is not in the app. It derefs to
/// the [`Zone`].
///
/// ```ignore
/// async fn report(zone: CurrentZone, range: Valid<Query<DayRange>>, uow: Uow) -> Result<WithZone<Report>> {
///     let (from, to) = range.utc_bounds(&zone);
///     let report = Report::between(&uow, from, to).await?;
///     Ok(WithZone::new(&zone, report))
/// }
/// ```
#[derive(Clone)]
pub struct CurrentZone {
    zone: Zone,
    level: Option<ZoneLevel>,
    owner: Option<Uuid>,
    from_country: bool,
    clock: Arc<dyn Clock>,
}

impl CurrentZone {
    pub(crate) fn new(
        zone: Zone,
        level: Option<ZoneLevel>,
        owner: Option<Uuid>,
        from_country: bool,
        clock: Arc<dyn Clock>,
    ) -> Self {
        CurrentZone {
            zone,
            level,
            owner,
            from_country,
            clock,
        }
    }

    /// The zone.
    pub fn zone(&self) -> Zone {
        self.zone
    }

    /// The level that gave the zone; `None` for the default zone.
    pub fn level(&self) -> Option<ZoneLevel> {
        self.level
    }

    /// The user, workspace or organization whose zone it is; `None` for
    /// the default zone.
    pub fn owner(&self) -> Option<Uuid> {
        self.owner
    }

    /// Whether the zone was derived from the owner's country (no zone was
    /// stored).
    pub fn from_country(&self) -> bool {
        self.from_country
    }

    /// The current time on the app's clock, in this zone.
    pub fn now(&self) -> DateTime<Tz> {
        self.zone.local(self.clock.now())
    }

    /// Today in this zone, on the app's clock.
    pub fn today(&self) -> LocalDay {
        self.zone.today(self.clock.now())
    }
}

impl Deref for CurrentZone {
    type Target = Zone;

    fn deref(&self) -> &Zone {
        &self.zone
    }
}

impl From<&CurrentZone> for Zone {
    fn from(current: &CurrentZone) -> Self {
        current.zone
    }
}

impl From<CurrentZone> for Zone {
    fn from(current: CurrentZone) -> Self {
        current.zone
    }
}

impl fmt::Debug for CurrentZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentZone")
            .field("zone", &self.zone)
            .field("level", &self.level)
            .field("owner", &self.owner)
            .field("from_country", &self.from_country)
            .finish()
    }
}

impl FromRequestParts for CurrentZone {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        if let Some(current) = parts.extensions.get::<CurrentZone>() {
            return Ok(current.clone());
        }
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("CurrentZone needs the navree request pipeline"))?;
        let zones = ctx
            .try_get::<Zones>()
            .await
            .ok_or_else(|| AppError::internal("CurrentZone needs TimeModule in the app"))?;
        let principal = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.clone());
        let current = {
            let mut request = ZoneRequest::new(principal.as_ref(), parts);
            zones
                .resolve(&mut request)
                .await
                .map_err(AppError::internal)?
        };
        parts.extensions.insert(current.clone());
        Ok(current)
    }
}

/// A response body with the zone its dates and days were counted in:
/// `data`'s fields plus `"timeZone": "Asia/Kolkata"` when `data` is an
/// object, else `{"data": data, "timeZone": ...}`. Front ends show dates in
/// that zone and send it back with day ranges.
///
/// ```
/// use navree_time::{WithZone, Zone};
///
/// let zone = Zone::parse("Asia/Kolkata").unwrap();
/// let body = serde_json::to_value(WithZone::new(zone, serde_json::json!({"sent": 12}))).unwrap();
/// assert_eq!(body, serde_json::json!({"sent": 12, "timeZone": "Asia/Kolkata"}));
/// let list = serde_json::to_value(WithZone::new(zone, vec![1, 2])).unwrap();
/// assert_eq!(list, serde_json::json!({"data": [1, 2], "timeZone": "Asia/Kolkata"}));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithZone<T> {
    /// The zone.
    pub zone: Zone,
    /// The body.
    pub data: T,
}

/// The key the zone is written under.
pub const TIME_ZONE_KEY: &str = "timeZone";

impl<T> WithZone<T> {
    /// `data` counted in `zone` (a `Zone`, `&Zone` or `&CurrentZone`).
    pub fn new(zone: impl Into<Zone>, data: T) -> Self {
        WithZone {
            zone: zone.into(),
            data,
        }
    }
}

impl<T: Serialize> Serialize for WithZone<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = serde_json::to_value(&self.data).map_err(S::Error::custom)?;
        let mut fields = match value {
            serde_json::Value::Object(fields) => fields,
            other => {
                let mut fields = serde_json::Map::new();
                fields.insert("data".to_owned(), other);
                fields
            }
        };
        fields.insert(TIME_ZONE_KEY.to_owned(), self.zone.name().into());
        fields.serialize(serializer)
    }
}

impl<T: Serialize> IntoResponse for WithZone<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}
