//! `GET /api/time/zones`: the zone list for zone pickers.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::Method;
use http::header::{CACHE_CONTROL, HeaderValue};
use navree_http::response::{IntoResponse, Response};
use navree_http::{ApiMeta, AppError, Controller, Json, Query, RouteDef, schema_fn};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{TimeOptions, Zone, Zones};

/// A zone as the zone list returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ZoneResource {
    /// The IANA name (`Asia/Kolkata`).
    pub name: String,
    /// The UTC offset in force now (`+05:30`).
    pub offset: String,
    /// The same offset in minutes east of UTC (`330`).
    pub offset_minutes: i32,
    /// The countries that use the zone (`["IN"]`).
    pub countries: Vec<String>,
}

impl ZoneResource {
    /// `zone` with its offset at `now`.
    pub fn new(zone: Zone, now: DateTime<Utc>) -> Self {
        let seconds = zone.offset_at(now).local_minus_utc();
        ZoneResource {
            name: zone.name().to_owned(),
            offset: zone.offset_label(now),
            offset_minutes: seconds / 60,
            countries: zone.countries().into_iter().map(str::to_owned).collect(),
        }
    }
}

/// `{"data": [...]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ZoneListResource {
    /// The zones, by name.
    pub data: Vec<ZoneResource>,
}

/// The query of `GET /api/time/zones`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ZoneListQuery {
    /// Only this country's zones (ISO 3166-1 alpha-2, any case), primary
    /// zone first.
    pub country: Option<String>,
    /// Only names containing this text (any case).
    pub search: Option<String>,
}

/// `GET {api_prefix}/time/zones?country=&search=`: every zone with its
/// current offset and countries, public (unless `time.routes` is false).
/// An unknown country is a 400 on `country`.
pub struct TimeController {
    zones: Arc<Zones>,
    options: Arc<TimeOptions>,
}

impl Injectable for TimeController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Zones>(), ProviderKey::of::<TimeOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(TimeController {
                zones: r.get::<Zones>().await?,
                options: r.get::<TimeOptions>().await?,
            })
        })
    }
}

impl Controller for TimeController {
    fn prefix(&self) -> &'static str {
        "/time"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.options.routes {
            return Vec::new();
        }
        let this = self.clone();
        vec![
            RouteDef::on(
                Method::GET,
                "/zones",
                move |Query(query): Query<ZoneListQuery>| {
                    let this = this.clone();
                    async move { this.zones(query) }
                },
            )
            .public()
            .name("time.zones")
            .api(ApiMeta {
                summary: Some("List the time zones with their current offsets".to_owned()),
                tags: vec!["Time".to_owned()],
                response: Some(schema_fn::<ZoneListResource>),
                ..ApiMeta::default()
            }),
        ]
    }
}

impl TimeController {
    fn zones(&self, query: ZoneListQuery) -> Result<Response, AppError> {
        let now = self.zones.now();
        let data = zone_list(query.country.as_deref(), query.search.as_deref())
            .map_err(|message| {
                AppError::validation(BTreeMap::from([("country".to_owned(), vec![message])]))
            })?
            .into_iter()
            .map(|zone| ZoneResource::new(zone, now))
            .collect();
        let mut response = Json(ZoneListResource { data }).into_response();
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        );
        Ok(response)
    }
}

/// The zones of `country` (primary first) or every zone (by name), whose
/// names contain `search`; an error message for an unknown country.
pub(crate) fn zone_list(country: Option<&str>, search: Option<&str>) -> Result<Vec<Zone>, String> {
    let zones = match country.map(str::trim).filter(|c| !c.is_empty()) {
        Some(country) => {
            let zones = Zone::in_country(country);
            if zones.is_empty() {
                return Err("country must be a valid ISO 3166-1 alpha-2 country code".to_owned());
            }
            zones
        }
        None => Zone::all().collect(),
    };
    let needle = search
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    Ok(zones
        .into_iter()
        .filter(|zone| {
            needle
                .as_deref()
                .is_none_or(|needle| zone.name().to_ascii_lowercase().contains(needle))
        })
        .collect())
}
