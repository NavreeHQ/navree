//! [`Zones`]: the app's zone service, resolving zones through every
//! `dyn ZoneSource`.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_contracts::{Clock, SystemClock, ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
use navree_kernel::BoxError;
use uuid::Uuid;

use crate::{CurrentZone, Holidays, Zone};

/// The app's zones: its default zone and holidays, and the zone sources
/// (users, workspaces, organizations) registered as `dyn ZoneSource`
/// multi-bindings.
///
/// Provided and exported by `TimeModule`. A zone resolves level by level,
/// user first, then workspace, then organization: the first owner with a
/// valid stored zone wins, or with a country (its primary zone); then the
/// default zone. A stored name that is not a zone is skipped with a
/// warning, so a bad row never breaks a request.
///
/// ```ignore
/// let zones = ctx.get::<Zones>().await?;
/// let zone = zones.zone_of(ZoneLevel::Workspace, workspace_id).await?;
/// let today = zone.today(zones.now());
/// ```
#[derive(Clone)]
pub struct Zones {
    default: Zone,
    holidays: Holidays,
    /// Sorted by level (stable: modules in dependency order within a level).
    sources: Vec<Arc<dyn ZoneSource>>,
    clock: Arc<dyn Clock>,
}

impl Zones {
    /// Zones with `default` and `sources` (any order), no holidays and the
    /// system clock.
    pub fn new(default: Zone, mut sources: Vec<Arc<dyn ZoneSource>>) -> Self {
        sources.sort_by_key(|source| source.level());
        Zones {
            default,
            holidays: Holidays::none(),
            sources,
            clock: Arc::new(SystemClock),
        }
    }

    /// Uses `holidays` as the app's holidays.
    pub fn with_holidays(mut self, holidays: Holidays) -> Self {
        self.holidays = holidays;
        self
    }

    /// Reads "now" from `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The app's default zone (`time.default_zone`).
    pub fn default_zone(&self) -> Zone {
        self.default
    }

    /// The app's holidays (`time.holidays`).
    pub fn holidays(&self) -> Holidays {
        self.holidays.clone()
    }

    /// The public holidays of `country` (built-in rules), for a tenant
    /// whose country differs from the app's.
    pub fn holidays_for(&self, country: &str) -> Holidays {
        Holidays::for_country(country)
    }

    /// The current time on the app's clock (`dyn Clock`).
    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// The app's clock.
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// The levels that have a source, in resolution order.
    pub fn levels(&self) -> Vec<ZoneLevel> {
        let mut levels: Vec<ZoneLevel> = self.sources.iter().map(|s| s.level()).collect();
        levels.dedup();
        levels
    }

    /// The zone of a request (or of a principal outside HTTP, with
    /// [`ZoneRequest::detached`]).
    pub async fn resolve(&self, request: &mut ZoneRequest<'_>) -> Result<CurrentZone, BoxError> {
        for source in &self.sources {
            if let Some(owner) = source.zone_of_request(request).await?
                && let Some((zone, from_country)) = self.zone_from(source.level(), &owner)
            {
                return Ok(CurrentZone::new(
                    zone,
                    Some(source.level()),
                    Some(owner.id),
                    from_country,
                    self.clock.clone(),
                ));
            }
        }
        Ok(CurrentZone::new(
            self.default,
            None,
            None,
            false,
            self.clock.clone(),
        ))
    }

    /// The zone of `owner` at `level` (its zone, else its country's primary
    /// zone), or the default zone.
    pub async fn zone_of(&self, level: ZoneLevel, owner: Uuid) -> Result<Zone, BoxError> {
        Ok(self
            .stored_zone_of(level, owner)
            .await?
            .unwrap_or(self.default))
    }

    /// The zone of `owner` at `level` (its zone, else its country's primary
    /// zone), or `None` when the owner has neither (where
    /// [`zone_of`](Zones::zone_of) gives the default zone): for code that
    /// falls back from one level to the next, such as a user's zone, else
    /// their workspace's.
    pub async fn stored_zone_of(
        &self,
        level: ZoneLevel,
        owner: Uuid,
    ) -> Result<Option<Zone>, BoxError> {
        for source in self.sources.iter().filter(|s| s.level() == level) {
            if let Some(stored) = source.zone_of(owner).await?
                && let Some((zone, _)) = self.zone_from(level, &stored)
            {
                return Ok(Some(zone));
            }
        }
        Ok(None)
    }

    /// Every owner at `level` with its zone (the default zone for owners
    /// without one), for work that runs per owner in local time.
    pub async fn owners(&self, level: ZoneLevel) -> Result<Vec<(Uuid, Zone)>, BoxError> {
        let mut owners: Vec<(Uuid, Zone)> = Vec::new();
        for source in self.sources.iter().filter(|s| s.level() == level) {
            for owner in source.owners().await? {
                if owners.iter().any(|(id, _)| *id == owner.id) {
                    continue;
                }
                let zone = self
                    .zone_from(level, &owner)
                    .map_or(self.default, |(zone, _)| zone);
                owners.push((owner.id, zone));
            }
        }
        Ok(owners)
    }

    /// The zone stored for `owner`: its zone if valid, else its country's
    /// primary zone (`true`), else `None`.
    pub fn zone_from(&self, level: ZoneLevel, owner: &ZoneOwner) -> Option<(Zone, bool)> {
        if let Some(name) = owner.zone.as_deref().filter(|n| !n.trim().is_empty()) {
            match Zone::parse(name) {
                Ok(zone) => return Some((zone, false)),
                Err(error) => tracing::warn!(
                    target: "navree::time",
                    %level,
                    owner = %owner.id,
                    %error,
                    "ignoring a stored time zone that is not a zone"
                ),
            }
        }
        let country = owner.country.as_deref()?;
        match Zone::for_country(country) {
            Some(zone) => Some((zone, true)),
            None => {
                tracing::warn!(
                    target: "navree::time",
                    %level,
                    owner = %owner.id,
                    country,
                    "ignoring a stored country that has no time zone"
                );
                None
            }
        }
    }
}

impl fmt::Debug for Zones {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Zones")
            .field("default", &self.default)
            .field("holidays", &self.holidays)
            .field("levels", &self.levels())
            .finish_non_exhaustive()
    }
}
