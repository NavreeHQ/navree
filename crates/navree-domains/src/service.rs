//! [`Domains`]: adding, verifying, finding and removing custom domains.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_kernel::AppContext;
use navree_orm::{Model as _, Record};
use rand::RngExt as _;
use uuid::Uuid;

use crate::events::{DomainAdded, DomainRemoved, DomainVerificationFailed, DomainVerified};
use crate::host::{is_within, normalize_host};
use crate::options::{DomainsOptions, TlsMode};
use crate::provider::RecordKind;
use crate::runtime::Runtime;
use crate::types::{CustomDomain, DnsInstruction, DomainStatus, NewDomain, TlsStatus};
use crate::verify::{self, Routing, txt_name};
use crate::{CurrentDomain, DomainCertificates, DomainsError, models};

/// Custom domains: workspaces on their own domains.
///
/// Provided by `DomainsModule` (`ctx.get::<Domains>()`, or as a handler
/// argument through the app context). A domain is added pending, with a
/// verification token; the scheduler (`domains:verify --due`) looks up its
/// TXT record, and whether it points at the app, until it is verified or
/// gives up. A verified domain names its workspace (the `domain`
/// resolver, [`CurrentDomain`]) and gets a certificate.
///
/// ```no_run
/// # async fn demo(ctx: navree_kernel::AppContext, workspace: uuid::Uuid) -> Result<(), navree_kernel::BoxError> {
/// use navree_domains::{Domains, NewDomain};
///
/// let domains = ctx.get::<Domains>().await?;
/// let domain = domains.add(workspace, NewDomain::new("crm.acme-corp.com")).await?;
/// for record in &domain.dns_records {
///     println!("{} {} {}", record.kind, record.name, record.value);
/// }
/// // Later, or from the scheduler:
/// let domain = domains.verify(domain.id).await?;
/// # let _ = domain;
/// # Ok(())
/// # }
/// ```
pub struct Domains {
    pub(crate) rt: Arc<Runtime>,
}

impl std::fmt::Debug for Domains {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Domains")
            .field("tls", &self.rt.options.tls)
            .finish_non_exhaustive()
    }
}

/// The length of a verification token.
const TOKEN_LEN: usize = 32;

/// A new verification token: lower-case letters and digits.
fn new_token() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..TOKEN_LEN)
        .map(|_| char::from(ALPHABET[rng.random_range(0..ALPHABET.len())]))
        .collect()
}

/// A purpose: lower-case letters, digits, `-` and `_`, 1 to 32.
fn check_purpose(purpose: &str) -> Result<(), DomainsError> {
    let valid = !purpose.is_empty()
        && purpose.len() <= 32
        && purpose
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(DomainsError::InvalidDomain(
            "purpose must be 1 to 32 lower-case letters, digits, `-` or `_`".to_owned(),
        ))
    }
}

impl Domains {
    /// Builds the service from the app (the module does this).
    pub(crate) async fn build(app: &AppContext) -> Result<Domains, DomainsError> {
        Ok(Domains {
            rt: Arc::new(Runtime::build(app).await?),
        })
    }

    /// The options.
    pub fn options(&self) -> &DomainsOptions {
        &self.rt.options
    }

    /// The certificates the app serves itself (`domains.tls = "acme"`): a
    /// rustls certificate resolver.
    pub fn certificates(&self) -> Arc<DomainCertificates> {
        self.rt.certificates.clone()
    }

    /// Whether `workspace.resolvers` lists `domain`, so requests to a
    /// verified domain name its workspace.
    pub fn resolves_workspaces(&self) -> bool {
        self.rt.resolves_workspaces
    }

    // -- adding ----------------------------------------------------------------

    /// Adds `input.host` to `workspace`. It is pending until its TXT
    /// record (and, when required, its routing) is found; a host inside
    /// `domains.dns.zone` gets its record from the DNS provider and is
    /// verified at once.
    ///
    /// Refused: a host that is not a domain (`invalid_domain`), a reserved
    /// one (`domain_not_allowed`), one another workspace has
    /// (`domain_taken`), or a workspace at `domains.max_per_workspace`
    /// (`domain_limit_reached`).
    pub async fn add(&self, workspace: Uuid, input: NewDomain) -> Result<CustomDomain, DomainsError> {
        let rt = self.rt.clone();
        let host = normalize_host(&input.host).map_err(DomainsError::InvalidDomain)?;
        let purpose = input
            .purpose
            .as_deref()
            .map(str::trim)
            .filter(|purpose| !purpose.is_empty())
            .unwrap_or("app")
            .to_owned();
        check_purpose(&purpose)?;
        let zone = rt.options.dns.zone.as_deref().and_then(|zone| normalize_host(zone).ok());
        let managed = zone
            .as_deref()
            .is_some_and(|zone| host != zone && is_within(&host, zone));
        if !managed
            && (rt.reserved.iter().any(|zone| is_within(&host, zone))
                || zone.as_deref() == Some(host.as_str()))
        {
            return Err(DomainsError::NotAllowed(host));
        }
        if rt.options.max_per_workspace > 0 {
            let count = models::Domain::query()
                .where_(models::Domain::WORKSPACE_ID.eq(workspace))
                .count(&*rt.db)
                .await?;
            if count >= u64::from(rt.options.max_per_workspace) {
                return Err(DomainsError::LimitReached(rt.options.max_per_workspace));
            }
        }
        if models::Domain::query()
            .where_(models::Domain::HOST.eq(host.clone()))
            .exists(&*rt.db)
            .await?
        {
            return Err(DomainsError::Taken(host));
        }
        if managed {
            self.set_managed_records(&host).await?;
        }
        let now = rt.now();
        let status = if managed {
            DomainStatus::Verified
        } else {
            DomainStatus::Pending
        };
        let row = models::Domain {
            id: Uuid::now_v7(),
            workspace_id: workspace,
            host: host.clone(),
            purpose,
            status: status.as_str().to_owned(),
            verification_token: new_token(),
            managed,
            is_primary: false,
            points_to_app: managed.then_some(true),
            check_attempts: 0,
            last_checked_at: None,
            next_check_at: if managed { self.recheck_at(now) } else { Some(now) },
            verified_at: managed.then_some(now),
            last_error: None,
            tls_status: self.tls_after(status).as_str().to_owned(),
            certificate_expires_at: None,
            certificate_checked_at: None,
            tls_error: None,
            created_by: input.created_by,
            created_at: now,
            updated_at: now,
        };
        let view = self.view(&row);
        let runtime = rt.clone();
        let event = view.clone();
        let inserted = rt
            .write(input.created_by, |uow| async move {
                let tx = uow.tx().await?;
                Record::new(row).save(&*tx).await?;
                uow.mark_modified();
                drop(tx);
                runtime
                    .emit_after_commit(&uow, DomainAdded {
                        domain: event.clone(),
                    })
                    .await;
                if managed {
                    runtime
                        .emit_after_commit(&uow, DomainVerified { domain: event })
                        .await;
                }
                Ok(())
            })
            .await;
        if let Err(error) = inserted {
            if managed {
                self.delete_managed_records(&host).await;
            }
            return Err(error);
        }
        rt.forget(&host).await;
        Ok(view)
    }

    /// The records a managed host gets: a CNAME to `domains.target`, or A
    /// and AAAA records of `domains.addresses`.
    async fn set_managed_records(&self, host: &str) -> Result<(), DomainsError> {
        let rt = &self.rt;
        let provider = rt.provider.as_ref().ok_or_else(|| {
            DomainsError::Config("domains.dns.zone needs a DNS provider".to_owned())
        })?;
        if let Some(target) = rt.options.target.as_deref() {
            return provider
                .set(RecordKind::Cname, host, &[target.to_owned()])
                .await;
        }
        let (v4, v6): (Vec<_>, Vec<_>) = rt.addresses.iter().partition(|ip| ip.is_ipv4());
        if v4.is_empty() && v6.is_empty() {
            return Err(DomainsError::Config(
                "a managed domain needs domains.target or domains.addresses".to_owned(),
            ));
        }
        let to_strings = |ips: Vec<&std::net::IpAddr>| ips.iter().map(ToString::to_string).collect::<Vec<_>>();
        if !v4.is_empty() {
            provider.set(RecordKind::A, host, &to_strings(v4)).await?;
        }
        if !v6.is_empty() {
            provider.set(RecordKind::Aaaa, host, &to_strings(v6)).await?;
        }
        Ok(())
    }

    /// Deletes a managed host's records; a failure is logged.
    async fn delete_managed_records(&self, host: &str) {
        let Some(provider) = &self.rt.provider else {
            return;
        };
        for kind in [RecordKind::Cname, RecordKind::A, RecordKind::Aaaa] {
            if let Err(error) = provider.delete(kind, host).await {
                tracing::warn!(target: "navree::domains", host, %error, "could not delete a DNS record of a removed domain");
            }
        }
    }

    // -- reading ---------------------------------------------------------------

    /// The domains of `workspace`, by host.
    pub async fn list(&self, workspace: Uuid) -> Result<Vec<CustomDomain>, DomainsError> {
        let rows = models::Domain::query()
            .where_(models::Domain::WORKSPACE_ID.eq(workspace))
            .order_by(models::Domain::HOST)
            .get(&*self.rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(|row| self.view(row)).collect())
    }

    /// Every domain, by host, optionally only those with `status`.
    pub async fn all(&self, status: Option<DomainStatus>) -> Result<Vec<CustomDomain>, DomainsError> {
        let mut query = models::Domain::query().order_by(models::Domain::HOST);
        if let Some(status) = status {
            query = query.where_(models::Domain::STATUS.eq(status.as_str()));
        }
        let rows = query.get(&*self.rt.db).await?.into_models();
        Ok(rows.iter().map(|row| self.view(row)).collect())
    }

    /// Domain `id`.
    pub async fn get(&self, id: Uuid) -> Result<CustomDomain, DomainsError> {
        Ok(self.view(&self.row(id).await?))
    }

    /// Domain `id` of `workspace` (`domain_not_found` for another
    /// workspace's).
    pub async fn get_for(&self, workspace: Uuid, id: Uuid) -> Result<CustomDomain, DomainsError> {
        let domain = self.get(id).await?;
        if domain.workspace_id == workspace {
            Ok(domain)
        } else {
            Err(DomainsError::DomainNotFound)
        }
    }

    /// The domain with host `host` (any status).
    pub async fn find(&self, host: &str) -> Result<Option<CustomDomain>, DomainsError> {
        let Ok(host) = normalize_host(host) else {
            return Ok(None);
        };
        let row = models::Domain::query()
            .where_(models::Domain::HOST.eq(host))
            .first(&*self.rt.db)
            .await?;
        Ok(row.as_ref().map(|row| self.view(row)))
    }

    /// The verified domain `host` names, from the cache when it is fresh
    /// (`domains.cache_secs`). `None` for any other host.
    pub async fn resolve(&self, host: &str) -> Result<Option<CurrentDomain>, DomainsError> {
        let Some(host) = crate::host::host_without_port(host) else {
            return Ok(None);
        };
        let Some(cache) = &self.rt.cache else {
            return self.lookup(&host).await;
        };
        cache
            .try_get_with(host.clone(), self.lookup(&host))
            .await
            .map_err(|error| match Arc::try_unwrap(error) {
                Ok(error) => error,
                Err(shared) => DomainsError::Config(shared.to_string()),
            })
    }

    async fn lookup(&self, host: &str) -> Result<Option<CurrentDomain>, DomainsError> {
        let row = models::Domain::query()
            .where_(models::Domain::HOST.eq(host))
            .where_(models::Domain::STATUS.eq(DomainStatus::Verified.as_str()))
            .first(&*self.rt.db)
            .await?;
        Ok(row.map(Record::into_inner).map(|row| CurrentDomain {
            id: row.id,
            workspace_id: row.workspace_id,
            host: row.host,
            purpose: row.purpose,
        }))
    }

    /// Whether an on-demand TLS proxy may get a certificate for `host`: it
    /// is a verified custom domain (the ask endpoint answers 200).
    pub async fn allows(&self, host: &str) -> Result<bool, DomainsError> {
        Ok(self.resolve(host).await?.is_some())
    }

    /// The primary domain of `workspace`, if it has one.
    pub async fn primary(&self, workspace: Uuid) -> Result<Option<CustomDomain>, DomainsError> {
        let row = models::Domain::query()
            .where_(models::Domain::WORKSPACE_ID.eq(workspace))
            .where_(models::Domain::IS_PRIMARY.eq(true))
            .first(&*self.rt.db)
            .await?;
        Ok(row.as_ref().map(|row| self.view(row)))
    }

    /// `https://` and the primary domain of `workspace`, for links in mail
    /// and pages (`None` without one).
    pub async fn primary_url(&self, workspace: Uuid) -> Result<Option<String>, DomainsError> {
        Ok(self
            .primary(workspace)
            .await?
            .filter(CustomDomain::is_verified)
            .map(|domain| format!("https://{}", domain.host)))
    }

    pub(crate) async fn row(&self, id: Uuid) -> Result<models::Domain, DomainsError> {
        models::Domain::query()
            .where_(models::Domain::ID.eq(id))
            .first(&*self.rt.db)
            .await?
            .map(Record::into_inner)
            .ok_or(DomainsError::DomainNotFound)
    }

    // -- changing --------------------------------------------------------------

    /// Makes verified domain `id` its workspace's primary domain (the
    /// previous one stops being primary).
    pub async fn make_primary(&self, id: Uuid) -> Result<CustomDomain, DomainsError> {
        let row = self.row(id).await?;
        if DomainStatus::parse(&row.status) != DomainStatus::Verified {
            return Err(DomainsError::NotVerified(row.host));
        }
        let now = self.rt.now();
        let workspace = row.workspace_id;
        self.rt
            .write(None, |uow| async move {
                let tx = uow.tx().await?;
                models::Domain::query()
                    .where_(models::Domain::WORKSPACE_ID.eq(workspace))
                    .where_(models::Domain::IS_PRIMARY.eq(true))
                    .update(
                        &*tx,
                        [
                            models::Domain::IS_PRIMARY.set(false),
                            models::Domain::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                models::Domain::query()
                    .where_(models::Domain::ID.eq(id))
                    .update(
                        &*tx,
                        [
                            models::Domain::IS_PRIMARY.set(true),
                            models::Domain::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                uow.mark_modified();
                Ok(())
            })
            .await?;
        self.get(id).await
    }

    /// Removes domain `id`: its row, its certificate, and (managed) its DNS
    /// records.
    pub async fn remove(&self, id: Uuid) -> Result<(), DomainsError> {
        let row = self.row(id).await?;
        let runtime = self.rt.clone();
        let host = row.host.clone();
        let event = DomainRemoved {
            domain_id: row.id,
            workspace_id: row.workspace_id,
            host: row.host.clone(),
        };
        self.rt
            .write(None, |uow| async move {
                let tx = uow.tx().await?;
                models::Domain::query()
                    .where_(models::Domain::ID.eq(id))
                    .delete(&*tx)
                    .await?;
                models::Certificate::query()
                    .where_(models::Certificate::HOST.eq(host))
                    .delete(&*tx)
                    .await?;
                uow.mark_modified();
                drop(tx);
                runtime.emit_after_commit(&uow, event).await;
                Ok(())
            })
            .await?;
        if row.managed {
            self.delete_managed_records(&row.host).await;
        }
        self.rt.certificates.remove(&row.host);
        self.rt.forget(&row.host).await;
        Ok(())
    }

    /// Removes every domain of `workspace` (when it is deleted); returns
    /// how many.
    pub async fn remove_workspace(&self, workspace: Uuid) -> Result<usize, DomainsError> {
        let ids: Vec<Uuid> = models::Domain::query()
            .where_(models::Domain::WORKSPACE_ID.eq(workspace))
            .get(&*self.rt.db)
            .await?
            .into_models()
            .into_iter()
            .map(|row| row.id)
            .collect();
        for id in &ids {
            self.remove(*id).await?;
        }
        Ok(ids.len())
    }

    // -- verification ----------------------------------------------------------

    /// Checks domain `id`'s DNS now: a pending or failed domain whose TXT
    /// record (and routing, when required) is found becomes verified; a
    /// verified one has its routing checked again. A resolver failure is
    /// recorded on the domain and returned (`dns_lookup_failed`).
    pub async fn verify(&self, id: Uuid) -> Result<CustomDomain, DomainsError> {
        let row = self.row(id).await?;
        self.check(row, false).await
    }

    /// Checks every domain whose next check is due (what
    /// `domains:verify --due` runs every minute); returns the domains
    /// checked. One domain's failure does not stop the others.
    pub async fn verify_due(&self) -> Result<Vec<CustomDomain>, DomainsError> {
        let now = self.rt.now();
        let rows = models::Domain::query()
            .where_(models::Domain::NEXT_CHECK_AT.lte(Some(now)))
            .where_(models::Domain::STATUS.ne(DomainStatus::Failed.as_str()))
            .order_by(models::Domain::NEXT_CHECK_AT)
            .limit(500)
            .get(&*self.rt.db)
            .await?
            .into_models();
        let mut checked = Vec::with_capacity(rows.len());
        for row in rows {
            let host = row.host.clone();
            match self.check(row, true).await {
                Ok(domain) => checked.push(domain),
                Err(error) => {
                    tracing::warn!(target: "navree::domains", host, %error, "a domain check failed");
                }
            }
        }
        Ok(checked)
    }

    async fn check(&self, row: models::Domain, scheduled: bool) -> Result<CustomDomain, DomainsError> {
        let rt = self.rt.clone();
        let routing = Routing {
            target: rt.options.target.as_deref(),
            addresses: &rt.addresses,
        };
        let result = verify::check(
            rt.dns.as_ref(),
            &row.host,
            &rt.options.verification.prefix,
            &row.verification_token,
            &routing,
            !row.managed,
        )
        .await;
        let now = rt.now();
        let mut row = row;
        row.last_checked_at = Some(now);
        row.updated_at = now;
        let was = DomainStatus::parse(&row.status);
        let mut verified = false;
        let mut gave_up = None;
        match &result {
            Err(error) => {
                row.last_error = Some(error.to_string());
                row.next_check_at = match was {
                    DomainStatus::Verified => self.recheck_at(now),
                    _ => Some(self.retry_at(now)),
                };
            }
            Ok(found) => {
                row.points_to_app = found.points_to_app;
                row.last_error = found.error();
                let passes = found.passes(rt.options.verification.require_routing);
                match was {
                    DomainStatus::Verified => {
                        row.next_check_at = self.recheck_at(now);
                        if let Some(error) = &row.last_error {
                            tracing::warn!(target: "navree::domains", host = row.host, error, "a verified domain no longer points at the app");
                        }
                    }
                    _ if passes => {
                        verified = true;
                        row.status = DomainStatus::Verified.as_str().to_owned();
                        row.verified_at = Some(now);
                        row.check_attempts = 0;
                        row.next_check_at = self.recheck_at(now);
                        row.tls_status = self.tls_after(DomainStatus::Verified).as_str().to_owned();
                    }
                    _ => {
                        row.check_attempts = row.check_attempts.saturating_add(1);
                        let deadline = row.created_at
                            + TimeDelta::hours(i64::from(rt.options.verification.give_up_after_hours));
                        if scheduled && was == DomainStatus::Pending && now >= deadline {
                            row.status = DomainStatus::Failed.as_str().to_owned();
                            row.next_check_at = None;
                            gave_up = Some(row.last_error.clone().unwrap_or_default());
                        } else if was == DomainStatus::Pending {
                            row.next_check_at = Some(self.retry_at(now));
                        }
                    }
                }
            }
        }
        let view = self.view(&row);
        let runtime = rt.clone();
        let event = view.clone();
        let saved = row.clone();
        let gave_up_reason = gave_up.clone();
        rt.write(None, |uow| async move {
            let tx = uow.tx().await?;
            let mut record = models::Domain::query()
                .where_(models::Domain::ID.eq(saved.id))
                .first(&*tx)
                .await?
                .ok_or(DomainsError::DomainNotFound)?;
            *record = saved;
            record.save(&*tx).await?;
            uow.mark_modified();
            drop(tx);
            if verified {
                runtime.emit_after_commit(&uow, DomainVerified { domain: event }).await;
            } else if let Some(reason) = gave_up_reason {
                runtime
                    .emit_after_commit(&uow, DomainVerificationFailed { domain: event, reason })
                    .await;
            }
            Ok(())
        })
        .await?;
        if verified || gave_up.is_some() {
            rt.forget(&row.host).await;
        }
        result.map(|_| view)
    }

    /// When a pending domain is checked next.
    fn retry_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now + TimeDelta::minutes(i64::from(self.rt.options.verification.interval_mins))
    }

    /// When a verified domain is checked next (`None`: never).
    fn recheck_at(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let hours = self.rt.options.verification.recheck_hours;
        (hours > 0).then(|| now + TimeDelta::hours(i64::from(hours)))
    }

    /// The certificate status of a domain with `status`.
    fn tls_after(&self, status: DomainStatus) -> TlsStatus {
        match (self.rt.options.tls, status) {
            (TlsMode::Off, _) => TlsStatus::Off,
            (TlsMode::OnDemand, DomainStatus::Verified) => TlsStatus::Active,
            _ => TlsStatus::Pending,
        }
    }

    // -- views -----------------------------------------------------------------

    /// The records `host` needs: the TXT record, then the CNAME (or A and
    /// AAAA records for an apex domain, which can't have a CNAME).
    pub fn dns_records(&self, host: &str, token: &str) -> Vec<DnsInstruction> {
        let options = &self.rt.options;
        let mut records = vec![DnsInstruction {
            kind: "TXT".to_owned(),
            name: txt_name(&options.verification.prefix, host),
            value: token.to_owned(),
            purpose: "verification".to_owned(),
        }];
        let apex = host.split('.').count() == 2;
        match options.target.as_deref() {
            Some(target) if !apex || self.rt.addresses.is_empty() => records.push(DnsInstruction {
                kind: "CNAME".to_owned(),
                name: host.to_owned(),
                value: target.to_owned(),
                purpose: "routing".to_owned(),
            }),
            _ => {
                for ip in &self.rt.addresses {
                    records.push(DnsInstruction {
                        kind: if ip.is_ipv4() { "A" } else { "AAAA" }.to_owned(),
                        name: host.to_owned(),
                        value: ip.to_string(),
                        purpose: "routing".to_owned(),
                    });
                }
            }
        }
        records
    }

    pub(crate) fn view(&self, row: &models::Domain) -> CustomDomain {
        CustomDomain {
            id: row.id,
            workspace_id: row.workspace_id,
            host: row.host.clone(),
            purpose: row.purpose.clone(),
            status: DomainStatus::parse(&row.status),
            primary: row.is_primary,
            managed: row.managed,
            points_to_app: row.points_to_app,
            tls: TlsStatus::parse(&row.tls_status),
            certificate_expires_at: row.certificate_expires_at,
            tls_error: row.tls_error.clone(),
            last_error: row.last_error.clone(),
            last_checked_at: row.last_checked_at,
            next_check_at: row.next_check_at,
            verified_at: row.verified_at,
            dns_records: if row.managed {
                Vec::new()
            } else {
                self.dns_records(&row.host, &row.verification_token)
            },
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_lower_case_letters_and_digits() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), TOKEN_LEN);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn purposes_are_short_names() {
        assert!(check_purpose("app").is_ok());
        assert!(check_purpose("landing-pages_2").is_ok());
        assert!(check_purpose("").is_err());
        assert!(check_purpose("Landing").is_err());
        assert!(check_purpose(&"x".repeat(33)).is_err());
    }
}
