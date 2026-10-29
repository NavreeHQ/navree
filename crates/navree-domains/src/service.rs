//! [`Domains`]: adding, verifying, finding and removing custom domains.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_db::Uow;
use navree_kernel::AppContext;
use navree_orm::{Assignment, Model as _, Record};
use rand::RngExt as _;
use uuid::Uuid;

use crate::events::{
    DomainAdded, DomainClaimedElsewhere, DomainRemoved, DomainVerificationFailed, DomainVerified,
};
use crate::host::{is_within, normalize_host};
use crate::options::{DomainsOptions, TlsMode};
use crate::provider::RecordKind;
use crate::runtime::Runtime;
use crate::savepoint::Savepoint;
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
/// Control of a domain is proved, not claimed. Typing a host adds a claim
/// that owns nothing, so any number of workspaces may hold a pending claim
/// on one host, each with a token of its own. The claim whose TXT record is
/// found first becomes verified, atomically (the database lets only one
/// claim own a host), and the other claims are closed with a
/// [`DomainClaimedElsewhere`] event. A verified domain moves to another
/// workspace only through [`transfer`](Domains::transfer), which needs a
/// fresh TXT proof of the workspace that takes it.
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

/// How a claim's try at owning its host ended.
enum Promotion {
    /// It owns the host now.
    Won,
    /// Another claim of the same domain verified it first.
    Already,
    /// Another claim owns the host: the database refused a second owner.
    Lost,
}

/// The columns a DNS check writes, and nothing else: a primary flag or a
/// certificate written meanwhile stays.
fn check_columns(row: &models::Domain) -> Vec<Assignment> {
    vec![
        models::Domain::STATUS.set(row.status.clone()),
        models::Domain::POINTS_TO_APP.set(row.points_to_app),
        models::Domain::CHECK_ATTEMPTS.set(row.check_attempts),
        models::Domain::LAST_CHECKED_AT.set(row.last_checked_at),
        models::Domain::NEXT_CHECK_AT.set(row.next_check_at),
        models::Domain::VERIFIED_AT.set(row.verified_at),
        models::Domain::LAST_ERROR.set(row.last_error.clone()),
        models::Domain::UPDATED_AT.set(row.updated_at),
    ]
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

    /// Adds a claim on `input.host` for `workspace`. It is pending, with a
    /// verification token of its own, until its TXT record (and, when
    /// required, its routing) is found; a host inside `domains.dns.zone`
    /// gets its record from the DNS provider and is verified at once.
    ///
    /// Other workspaces' pending and failed claims on the host never get
    /// in the way: the claim that proves control wins. A workspace that
    /// had a failed claim on the host replaces it with this one.
    ///
    /// Refused: a host that is not a domain (`invalid_domain`), a reserved
    /// one (`domain_not_allowed`), one that is verified for a workspace
    /// already, or that this workspace has claimed (`domain_taken`), or a
    /// workspace at `domains.max_per_workspace` (`domain_limit_reached`;
    /// failed claims don't count).
    pub async fn add(
        &self,
        workspace: Uuid,
        input: NewDomain,
    ) -> Result<CustomDomain, DomainsError> {
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
        let managed = self.admit(&host)?;
        self.check_limit(workspace).await?;
        if self.verified_row(&host).await?.is_some() {
            return Err(DomainsError::Taken(host));
        }
        let mut replaced = None;
        if let Some(mine) = self.claim_row(workspace, &host).await? {
            if DomainStatus::parse(&mine.status) != DomainStatus::Failed {
                return Err(DomainsError::Taken(host));
            }
            replaced = Some(mine.id);
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
            verified_host: managed.then(|| host.clone()),
            verification_token: new_token(),
            transfer: false,
            managed,
            is_primary: false,
            points_to_app: managed.then_some(true),
            check_attempts: 0,
            last_checked_at: None,
            next_check_at: if managed {
                self.recheck_at(now)
            } else {
                Some(now)
            },
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
                if let Some(old) = replaced {
                    models::Domain::query()
                        .where_(models::Domain::ID.eq(old))
                        .delete(&uow)
                        .await?;
                }
                Record::new(row).save(&uow).await?;
                uow.mark_modified();
                runtime
                    .emit_after_commit(
                        &uow,
                        DomainAdded {
                            domain: event.clone(),
                        },
                    )
                    .await;
                if managed {
                    runtime
                        .emit_after_commit(
                            &uow,
                            DomainVerified {
                                domain: event.clone(),
                            },
                        )
                        .await;
                    Domains::close_claims_after_commit(&runtime, &uow, event);
                }
                Ok(())
            })
            .await;
        if let Err(error) = inserted {
            // Another claim of this workspace, or another workspace's
            // managed domain, got there first. The DNS records are theirs
            // too (the same host, the same target): leave them.
            if error.is_unique_violation() {
                return Err(DomainsError::Taken(host));
            }
            if managed {
                self.delete_managed_records(&host).await;
            }
            return Err(error);
        }
        rt.forget(&host);
        Ok(view)
    }

    /// Moves the verified domain `host` to workspace `to`, once `to` proves
    /// control of it with a fresh TXT record.
    ///
    /// A verified domain never moves on a claim alone, and the TXT record
    /// its current workspace published proves nothing for `to`. This
    /// makes `to` a claim of its own, marked as a transfer, with a new
    /// token (the same one when called again, until it is verified), and
    /// checks the DNS now: with the record in place the domain moves at
    /// once, and the old workspace's domain and certificate are removed
    /// (it gets a [`DomainClaimedElsewhere`] event). Without it, the
    /// returned claim is pending, with the records to create in
    /// `dns_records`; the scheduler completes the move when it finds them.
    ///
    /// Refused: a host that is not a domain (`invalid_domain`), a reserved
    /// or managed one (`domain_not_allowed`), one nobody has verified
    /// (`domain_not_found`: add it instead), or a workspace at its limit
    /// (`domain_limit_reached`). A host `to` holds already is returned as
    /// it is. A resolver failure is `dns_lookup_failed`.
    pub async fn transfer(&self, host: &str, to: Uuid) -> Result<CustomDomain, DomainsError> {
        let host = normalize_host(host).map_err(DomainsError::InvalidDomain)?;
        if self.admit(&host)? {
            // A managed domain has no proof to give: remove it and add it.
            return Err(DomainsError::NotAllowed(host));
        }
        let Some(holder) = self.verified_row(&host).await? else {
            return Err(DomainsError::DomainNotFound);
        };
        if holder.workspace_id == to {
            return Ok(self.view(&holder));
        }
        let existing = self.claim_row(to, &host).await?;
        let reusable = existing.as_ref().is_some_and(|row| {
            row.transfer && DomainStatus::parse(&row.status) == DomainStatus::Pending
        });
        let claim = match existing {
            Some(row) if reusable => row,
            existing => {
                if existing
                    .as_ref()
                    .is_none_or(|row| DomainStatus::parse(&row.status) == DomainStatus::Failed)
                {
                    self.check_limit(to).await?;
                }
                self.start_transfer(to, &host, existing).await?
            }
        };
        self.check(claim, false).await
    }

    /// Stores `to`'s transfer claim on `host`: a new row, or `existing`
    /// started over with a fresh token.
    async fn start_transfer(
        &self,
        to: Uuid,
        host: &str,
        existing: Option<models::Domain>,
    ) -> Result<models::Domain, DomainsError> {
        let rt = self.rt.clone();
        let now = rt.now();
        let created = existing.is_none();
        let mut row = existing.unwrap_or_else(|| models::Domain {
            id: Uuid::now_v7(),
            workspace_id: to,
            host: host.to_owned(),
            purpose: "app".to_owned(),
            ..models::Domain::default()
        });
        row.status = DomainStatus::Pending.as_str().to_owned();
        row.verified_host = None;
        row.verification_token = new_token();
        row.transfer = true;
        row.managed = false;
        row.points_to_app = None;
        row.check_attempts = 0;
        row.last_checked_at = None;
        row.next_check_at = Some(now);
        row.verified_at = None;
        row.last_error = None;
        row.tls_status = self.tls_after(DomainStatus::Pending).as_str().to_owned();
        row.created_at = now;
        row.updated_at = now;
        let saved = row.clone();
        let event = self.view(&row);
        let runtime = rt.clone();
        let stored = rt
            .write(None, |uow| async move {
                if created {
                    Record::new(saved).save(&uow).await?;
                } else {
                    models::Domain::query()
                        .where_(models::Domain::ID.eq(saved.id))
                        .update(
                            &uow,
                            [
                                models::Domain::STATUS.set(saved.status),
                                models::Domain::VERIFICATION_TOKEN.set(saved.verification_token),
                                models::Domain::TRANSFER.set(true),
                                models::Domain::POINTS_TO_APP.set(None::<bool>),
                                models::Domain::CHECK_ATTEMPTS.set(0),
                                models::Domain::LAST_CHECKED_AT.set(None::<DateTime<Utc>>),
                                models::Domain::NEXT_CHECK_AT.set(saved.next_check_at),
                                models::Domain::LAST_ERROR.set(None::<String>),
                                models::Domain::TLS_STATUS.set(saved.tls_status),
                                models::Domain::CREATED_AT.set(saved.created_at),
                                models::Domain::UPDATED_AT.set(saved.updated_at),
                            ],
                        )
                        .await?;
                }
                uow.mark_modified();
                if created {
                    runtime
                        .emit_after_commit(&uow, DomainAdded { domain: event })
                        .await;
                }
                Ok(())
            })
            .await;
        match stored {
            Ok(()) => Ok(row),
            Err(error) if error.is_unique_violation() => Err(DomainsError::Taken(row.host)),
            Err(error) => Err(error),
        }
    }

    /// Whether `host` is inside the managed zone (`true`); `NotAllowed`
    /// when it is reserved: the app's own domains and their subdomains.
    fn admit(&self, host: &str) -> Result<bool, DomainsError> {
        let rt = &self.rt;
        let zone = rt
            .options
            .dns
            .zone
            .as_deref()
            .and_then(|zone| normalize_host(zone).ok());
        let managed = zone
            .as_deref()
            .is_some_and(|zone| host != zone && is_within(host, zone));
        if !managed
            && (rt.reserved.iter().any(|zone| is_within(host, zone))
                || zone.as_deref() == Some(host))
        {
            return Err(DomainsError::NotAllowed(host.to_owned()));
        }
        Ok(managed)
    }

    /// `LimitReached` when `workspace` has `domains.max_per_workspace`
    /// domains. Failed claims are not counted: they own nothing.
    async fn check_limit(&self, workspace: Uuid) -> Result<(), DomainsError> {
        let rt = &self.rt;
        if rt.options.max_per_workspace == 0 {
            return Ok(());
        }
        let count = models::Domain::query()
            .where_(models::Domain::WORKSPACE_ID.eq(workspace))
            .where_(models::Domain::STATUS.ne(DomainStatus::Failed.as_str()))
            .count(&*rt.db)
            .await?;
        if count >= u64::from(rt.options.max_per_workspace) {
            return Err(DomainsError::LimitReached(rt.options.max_per_workspace));
        }
        Ok(())
    }

    /// The verified domain that owns `host`, if any.
    async fn verified_row(&self, host: &str) -> Result<Option<models::Domain>, DomainsError> {
        Ok(models::Domain::query()
            .where_(models::Domain::VERIFIED_HOST.eq(host))
            .first(&*self.rt.db)
            .await?
            .map(Record::into_inner))
    }

    /// `workspace`'s claim on `host`, in any status.
    async fn claim_row(
        &self,
        workspace: Uuid,
        host: &str,
    ) -> Result<Option<models::Domain>, DomainsError> {
        Ok(models::Domain::query()
            .where_(models::Domain::WORKSPACE_ID.eq(workspace))
            .where_(models::Domain::HOST.eq(host))
            .first(&*self.rt.db)
            .await?
            .map(Record::into_inner))
    }

    /// Every claim on `host`: the verified one first, then the pending
    /// ones, then the failed ones, the newest first among equals.
    async fn claim_rows(&self, host: &str) -> Result<Vec<models::Domain>, DomainsError> {
        let mut rows: Vec<models::Domain> = models::Domain::query()
            .where_(models::Domain::HOST.eq(host))
            .get(&*self.rt.db)
            .await?
            .into_models();
        let rank = |row: &models::Domain| match DomainStatus::parse(&row.status) {
            DomainStatus::Verified => 0,
            DomainStatus::Pending => 1,
            DomainStatus::Failed => 2,
        };
        rows.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        Ok(rows)
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
        let to_strings =
            |ips: Vec<&std::net::IpAddr>| ips.iter().map(ToString::to_string).collect::<Vec<_>>();
        if !v4.is_empty() {
            provider.set(RecordKind::A, host, &to_strings(v4)).await?;
        }
        if !v6.is_empty() {
            provider
                .set(RecordKind::Aaaa, host, &to_strings(v6))
                .await?;
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
    pub async fn all(
        &self,
        status: Option<DomainStatus>,
    ) -> Result<Vec<CustomDomain>, DomainsError> {
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

    /// The domain that holds `host`: its verified domain, or else the
    /// newest pending claim, or else the newest failed one. `None` for a
    /// host nobody has claimed. [`claims`](Domains::claims) lists them all.
    pub async fn find(&self, host: &str) -> Result<Option<CustomDomain>, DomainsError> {
        Ok(self.claims(host).await?.into_iter().next())
    }

    /// Every claim on `host`, in any status: the verified domain first,
    /// then the pending claims, then the failed ones. Empty for a host
    /// that is not a domain.
    pub async fn claims(&self, host: &str) -> Result<Vec<CustomDomain>, DomainsError> {
        let Ok(host) = normalize_host(host) else {
            return Ok(Vec::new());
        };
        let rows = self.claim_rows(&host).await?;
        Ok(rows.iter().map(|row| self.view(row)).collect())
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
        // Only a verified domain owns its host.
        let row = self.verified_row(host).await?;
        Ok(row.map(|row| CurrentDomain {
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
                models::Domain::query()
                    .where_(models::Domain::WORKSPACE_ID.eq(workspace))
                    .where_(models::Domain::IS_PRIMARY.eq(true))
                    .update(
                        &uow,
                        [
                            models::Domain::IS_PRIMARY.set(false),
                            models::Domain::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                models::Domain::query()
                    .where_(models::Domain::ID.eq(id))
                    .update(
                        &uow,
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

    /// Removes domain `id`: its row and, when it was the verified domain
    /// of its host, its certificate and (managed) its DNS records. A
    /// pending or failed claim owns neither: removing it leaves the
    /// verified domain's certificate alone.
    pub async fn remove(&self, id: Uuid) -> Result<(), DomainsError> {
        let row = self.row(id).await?;
        let held = DomainStatus::parse(&row.status) == DomainStatus::Verified;
        let runtime = self.rt.clone();
        let host = row.host.clone();
        let event = DomainRemoved {
            domain_id: row.id,
            workspace_id: row.workspace_id,
            host: row.host.clone(),
        };
        self.rt
            .write(None, |uow| async move {
                models::Domain::query()
                    .where_(models::Domain::ID.eq(id))
                    .delete(&uow)
                    .await?;
                if held {
                    models::Certificate::query()
                        .where_(models::Certificate::HOST.eq(host))
                        .delete(&uow)
                        .await?;
                }
                uow.mark_modified();
                runtime.emit_after_commit(&uow, event).await;
                Ok(())
            })
            .await?;
        if held {
            if row.managed {
                self.delete_managed_records(&row.host).await;
            }
            self.rt.certificates.remove(&row.host);
        }
        self.rt.forget(&row.host);
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
    /// checked. One domain's failure does not stop the others. Failed
    /// claims older than `domains.verification.failed_retention_hours` are
    /// deleted first.
    pub async fn verify_due(&self) -> Result<Vec<CustomDomain>, DomainsError> {
        self.purge_failed().await?;
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

    async fn check(
        &self,
        row: models::Domain,
        scheduled: bool,
    ) -> Result<CustomDomain, DomainsError> {
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
                            + TimeDelta::hours(i64::from(
                                rt.options.verification.give_up_after_hours,
                            ));
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
        if verified {
            // A domain that proves control competes for its host.
            return self.promote(row).await;
        }
        let view = self.view(&row);
        let runtime = rt.clone();
        let event = view.clone();
        let saved = row.clone();
        let gave_up_reason = gave_up.clone();
        rt.write(None, |uow| async move {
            // Only the check's columns: a primary flag or a certificate
            // written meanwhile stays.
            let updated = models::Domain::query()
                .where_(models::Domain::ID.eq(saved.id))
                .update(&uow, check_columns(&saved))
                .await?;
            if updated == 0 {
                return Err(DomainsError::DomainNotFound);
            }
            uow.mark_modified();
            if let Some(reason) = gave_up_reason {
                runtime
                    .emit_after_commit(
                        &uow,
                        DomainVerificationFailed {
                            domain: event,
                            reason,
                        },
                    )
                    .await;
            }
            Ok(())
        })
        .await?;
        if gave_up.is_some() {
            rt.forget(&row.host);
        }
        result.map(|_| view)
    }

    /// Makes `row`, a pending or failed claim whose proof was just found
    /// (it carries its verified state), the owner of its host.
    ///
    /// The database decides: only one claim can set `verified_host`, so of
    /// two claims proving control at once exactly one commits and the other
    /// finds the host taken. The winner closes the other pending claims
    /// once it has committed. A host another workspace already owns stays
    /// with it, unless `row` is a transfer: then the proof moves the host,
    /// and the old domain and its certificate go in the same transaction.
    async fn promote(&self, row: models::Domain) -> Result<CustomDomain, DomainsError> {
        let rt = self.rt.clone();
        let mut superseded = None;
        if let Some(holder) = self.verified_row(&row.host).await? {
            if holder.id == row.id {
                // Verified meanwhile, by another check of the same claim.
                return Ok(self.view(&holder));
            }
            if !row.transfer {
                return self.lose(row.id, holder.workspace_id).await;
            }
            superseded = Some(holder);
        }
        let view = self.view(&row);
        let moved_from = superseded.as_ref().map(|old| self.view(old));
        let old_id = superseded.as_ref().map(|old| old.id);
        let claimed_by = row.workspace_id;
        let runtime = rt.clone();
        let event = view.clone();
        let saved = row.clone();
        let outcome = rt
            .write(None, |uow| async move {
                // A savepoint, so that losing the race leaves the unit of
                // work (a request's, perhaps) usable on PostgreSQL.
                let attempt = Savepoint::begin(&uow, "promote_domain").await?;
                let claimed = async {
                    if let Some(old) = old_id {
                        models::Domain::query()
                            .where_(models::Domain::ID.eq(old))
                            .delete(&uow)
                            .await?;
                        models::Certificate::query()
                            .where_(models::Certificate::HOST.eq(saved.host.clone()))
                            .delete(&uow)
                            .await?;
                    }
                    let mut columns = check_columns(&saved);
                    columns.push(models::Domain::TLS_STATUS.set(saved.tls_status.clone()));
                    columns.push(models::Domain::VERIFIED_HOST.set(Some(saved.host.clone())));
                    columns.push(models::Domain::TRANSFER.set(false));
                    models::Domain::query()
                        .where_(models::Domain::ID.eq(saved.id))
                        .where_(models::Domain::STATUS.ne(DomainStatus::Verified.as_str()))
                        .update(&uow, columns)
                        .await
                }
                .await;
                match claimed {
                    Ok(0) => {
                        attempt.rollback().await?;
                        Ok(Promotion::Already)
                    }
                    Ok(_) => {
                        attempt.release().await?;
                        uow.mark_modified();
                        runtime
                            .emit_after_commit(
                                &uow,
                                DomainVerified {
                                    domain: event.clone(),
                                },
                            )
                            .await;
                        if let Some(old) = moved_from {
                            runtime
                                .emit_after_commit(
                                    &uow,
                                    DomainClaimedElsewhere {
                                        domain: old,
                                        claimed_by,
                                    },
                                )
                                .await;
                        }
                        Domains::close_claims_after_commit(&runtime, &uow, event);
                        Ok(Promotion::Won)
                    }
                    Err(error) => {
                        attempt.rollback().await?;
                        let error = DomainsError::from(error);
                        if error.is_unique_violation() {
                            Ok(Promotion::Lost)
                        } else {
                            Err(error)
                        }
                    }
                }
            })
            .await?;
        rt.forget(&row.host);
        match outcome {
            Promotion::Won => {
                if old_id.is_some() {
                    rt.certificates.remove(&row.host);
                }
                Ok(view)
            }
            Promotion::Already => self.get(row.id).await,
            Promotion::Lost => {
                let holder = self.verified_row(&row.host).await?;
                match holder {
                    Some(holder) => self.lose(row.id, holder.workspace_id).await,
                    // The winner went away again meanwhile: try next time.
                    None => self.get(row.id).await,
                }
            }
        }
    }

    /// Closes claim `id`, which lost its host to `claimed_by`.
    async fn lose(&self, id: Uuid, claimed_by: Uuid) -> Result<CustomDomain, DomainsError> {
        let row = self.row(id).await?;
        self.close_claim(row, claimed_by).await
    }

    /// Closes the pending claim `row` as failed, with the reason in its
    /// `last_error` and a [`DomainClaimedElsewhere`] event (once: when it
    /// was already closed, nothing is emitted). Returns the claim as it is
    /// now.
    async fn close_claim(
        &self,
        row: models::Domain,
        claimed_by: Uuid,
    ) -> Result<CustomDomain, DomainsError> {
        let rt = self.rt.clone();
        let now = rt.now();
        let reason = format!(
            "{} is verified for another workspace: its owner proved control of it first",
            row.host
        );
        let mut closed = row;
        closed.status = DomainStatus::Failed.as_str().to_owned();
        closed.next_check_at = None;
        closed.last_error = Some(reason.clone());
        closed.updated_at = now;
        let view = self.view(&closed);
        let runtime = rt.clone();
        let event = view.clone();
        let id = closed.id;
        rt.write(None, |uow| async move {
            let updated = models::Domain::query()
                .where_(models::Domain::ID.eq(id))
                .where_(models::Domain::STATUS.eq(DomainStatus::Pending.as_str()))
                .update(
                    &uow,
                    [
                        models::Domain::STATUS.set(DomainStatus::Failed.as_str()),
                        models::Domain::NEXT_CHECK_AT.set(None::<DateTime<Utc>>),
                        models::Domain::LAST_ERROR.set(Some(reason.clone())),
                        models::Domain::UPDATED_AT.set(now),
                    ],
                )
                .await?;
            if updated == 0 {
                // Failed before (or already closed by the winner): say why.
                models::Domain::query()
                    .where_(models::Domain::ID.eq(id))
                    .where_(models::Domain::STATUS.eq(DomainStatus::Failed.as_str()))
                    .update(&uow, [models::Domain::LAST_ERROR.set(Some(reason))])
                    .await?;
            } else {
                runtime
                    .emit_after_commit(
                        &uow,
                        DomainClaimedElsewhere {
                            domain: event,
                            claimed_by,
                        },
                    )
                    .await;
            }
            uow.mark_modified();
            Ok(())
        })
        .await?;
        self.get(id).await.or(Ok(view))
    }

    /// Closes the other pending claims on `winner`'s host (see
    /// [`Domains::close_claims`]) once `uow` commits: while the winner's
    /// transaction is open, a claim being verified at the same time would
    /// wait on it, and the winner on the claim.
    fn close_claims_after_commit(rt: &Arc<Runtime>, uow: &Uow, winner: CustomDomain) {
        let rt = rt.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                let domains = Domains { rt };
                if let Err(error) = domains.close_claims(&winner).await {
                    tracing::warn!(target: "navree::domains", host = winner.host, %error, "the other claims on a verified host could not be closed");
                }
            })
        });
    }

    /// Closes every pending claim on `winner`'s host except `winner`.
    async fn close_claims(&self, winner: &CustomDomain) -> Result<(), DomainsError> {
        let others: Vec<models::Domain> = models::Domain::query()
            .where_(models::Domain::HOST.eq(winner.host.clone()))
            .where_(models::Domain::ID.ne(winner.id))
            .where_(models::Domain::STATUS.eq(DomainStatus::Pending.as_str()))
            .get(&*self.rt.db)
            .await?
            .into_models();
        for row in others {
            self.close_claim(row, winner.workspace_id).await?;
        }
        Ok(())
    }

    /// Deletes the failed claims that last changed more than
    /// `domains.verification.failed_retention_hours` ago; returns how many.
    async fn purge_failed(&self) -> Result<u64, DomainsError> {
        let rt = &self.rt;
        let hours = rt.options.verification.failed_retention_hours;
        if hours == 0 {
            return Ok(0);
        }
        let before = rt.now() - TimeDelta::hours(i64::from(hours));
        let stale = || {
            models::Domain::query()
                .where_(models::Domain::STATUS.eq(DomainStatus::Failed.as_str()))
                .where_(models::Domain::UPDATED_AT.lte(before))
        };
        // The usual minute has nothing to delete: don't open a transaction.
        if !stale().exists(&*rt.db).await? {
            return Ok(0);
        }
        rt.write(None, |uow| async move {
            let deleted = stale().delete(&uow).await?;
            uow.mark_modified();
            Ok(deleted)
        })
        .await
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
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
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
