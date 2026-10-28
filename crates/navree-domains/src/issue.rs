//! Certificates: issuing and renewing them through ACME, serving them,
//! probing the on-demand proxy's, and reporting the ones close to expiry.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_kernel::BoxFuture;
use navree_orm::{Model as _, Record};
use uuid::Uuid;

use crate::acme::{AcmeOrder, AcmeStore};
use crate::certs::{certified_key, validity};
use crate::events::{DomainCertificateExpiring, DomainCertificateIssued};
use crate::host::normalize_host;
use crate::options::TlsMode;
use crate::runtime::Runtime;
use crate::types::{CertificateRun, DomainStatus, TlsStatus};
use crate::{Domains, DomainsError, models};

/// How long a published challenge answer is served at most.
const CHALLENGE_TTL_MINUTES: i64 = 60;

impl Domains {
    /// Gets a certificate for `host` from the ACME CA now (a verified
    /// custom domain, or one of `domains.acme.hosts`), stores it with its
    /// key sealed, serves it, and returns when it expires. Needs
    /// `domains.tls = "acme"` and an application key.
    pub async fn issue_certificate(&self, host: &str) -> Result<DateTime<Utc>, DomainsError> {
        let rt = self.rt.clone();
        if rt.options.tls != TlsMode::Acme {
            return Err(DomainsError::Config(
                "certificates are issued with domains.tls = \"acme\"".to_owned(),
            ));
        }
        let host = normalize_host(host).map_err(DomainsError::InvalidDomain)?;
        let domain = models::Domain::query()
            .where_(models::Domain::HOST.eq(host.clone()))
            .first(&*rt.db)
            .await?;
        let own = rt
            .options
            .acme
            .hosts
            .iter()
            .filter_map(|own| normalize_host(own).ok())
            .any(|own| own == host);
        match &domain {
            Some(row) if DomainStatus::parse(&row.status) != DomainStatus::Verified => {
                return Err(DomainsError::NotVerified(host));
            }
            None if !own => return Err(DomainsError::DomainNotFound),
            _ => {}
        }
        // The key must be sealable before a certificate is ordered.
        rt.seal(&host)?;
        let order = AcmeOrder {
            host: host.clone(),
            directory_url: rt.options.acme.directory_url.clone(),
            contact: rt.options.acme.contact.clone(),
            store: Arc::new(Store(rt.clone())),
        };
        let issued = rt.acme.issue(&order).await.and_then(|issued| {
            certified_key(&issued.chain_pem, &issued.private_key_pem)?;
            let validity = validity(&issued.chain_pem)?;
            Ok((issued, validity))
        });
        let now = rt.now();
        let domain_id = domain.as_ref().map(|row| row.id);
        let (issued, validity) = match issued {
            Ok(found) => found,
            Err(error) => {
                if let Some(id) = domain_id {
                    models::Domain::query()
                        .where_(models::Domain::ID.eq(id))
                        .update(
                            &*rt.db,
                            [
                                models::Domain::TLS_STATUS.set(TlsStatus::Failed.as_str()),
                                models::Domain::TLS_ERROR.set(Some(error.to_string())),
                                models::Domain::CERTIFICATE_CHECKED_AT.set(Some(now)),
                                models::Domain::UPDATED_AT.set(now),
                            ],
                        )
                        .await?;
                }
                tracing::warn!(target: "navree::domains", host, %error, "a certificate could not be issued");
                return Err(error);
            }
        };
        let sealed = rt.seal(&issued.private_key_pem)?;
        let runtime = rt.clone();
        let chain = issued.chain_pem.clone();
        let stored_host = host.clone();
        let renewal = rt
            .write(None, |uow| async move {
                let tx = uow.tx().await?;
                let existing = models::Certificate::query()
                    .where_(models::Certificate::HOST.eq(stored_host.clone()))
                    .first(&*tx)
                    .await?;
                let renewal = existing.is_some();
                let mut record = existing.unwrap_or_else(|| {
                    Record::new(models::Certificate {
                        id: Uuid::now_v7(),
                        host: stored_host.clone(),
                        created_at: now,
                        ..models::Certificate::default()
                    })
                });
                record.domain_id = domain_id;
                record.chain_pem = chain;
                record.private_key = sealed;
                record.not_before = validity.not_before;
                record.not_after = validity.not_after;
                record.updated_at = now;
                record.save(&*tx).await?;
                if let Some(id) = domain_id {
                    models::Domain::query()
                        .where_(models::Domain::ID.eq(id))
                        .update(
                            &*tx,
                            [
                                models::Domain::TLS_STATUS.set(TlsStatus::Active.as_str()),
                                models::Domain::TLS_ERROR.set(None::<String>),
                                models::Domain::CERTIFICATE_EXPIRES_AT.set(Some(validity.not_after)),
                                models::Domain::CERTIFICATE_CHECKED_AT.set(Some(now)),
                                models::Domain::UPDATED_AT.set(now),
                            ],
                        )
                        .await?;
                }
                uow.mark_modified();
                drop(tx);
                runtime
                    .emit_after_commit(
                        &uow,
                        DomainCertificateIssued {
                            host: stored_host,
                            domain_id,
                            expires_at: validity.not_after,
                            renewal,
                        },
                    )
                    .await;
                Ok(renewal)
            })
            .await?;
        rt.certificates
            .insert_pem(&host, &issued.chain_pem, &issued.private_key_pem)?;
        tracing::info!(target: "navree::domains", host, renewal, expires_at = %validity.not_after, "certificate issued");
        Ok(validity.not_after)
    }

    /// Loads every stored certificate into [`certificates`](Self::certificates)
    /// (the HTTPS listener does this at start and every
    /// `domains.acme.reload_secs`, so each server sees the others'
    /// renewals); returns how many it serves.
    pub async fn reload_certificates(&self) -> Result<usize, DomainsError> {
        let rt = &self.rt;
        let rows = models::Certificate::query()
            .get(&*rt.db)
            .await?
            .into_models();
        let mut hosts = Vec::with_capacity(rows.len());
        for row in rows {
            let loaded = rt
                .open(&row.private_key)
                .and_then(|key| rt.certificates.insert_pem(&row.host, &row.chain_pem, &key));
            match loaded {
                Ok(()) => hosts.push(row.host),
                Err(error) => {
                    tracing::warn!(target: "navree::domains", host = row.host, %error, "a stored certificate could not be loaded");
                }
            }
        }
        for served in rt.certificates.hosts() {
            if !hosts.contains(&served) {
                rt.certificates.remove(&served);
            }
        }
        Ok(hosts.len())
    }

    /// Issues the certificates of newly verified domains (`tls = "acme"`;
    /// what `domains:verify --due` runs after its checks).
    pub async fn issue_pending(&self) -> Result<Vec<String>, DomainsError> {
        if self.rt.options.tls != TlsMode::Acme {
            return Ok(Vec::new());
        }
        let hosts: Vec<String> = models::Domain::query()
            .where_(models::Domain::STATUS.eq(DomainStatus::Verified.as_str()))
            .where_(models::Domain::TLS_STATUS.eq(TlsStatus::Pending.as_str()))
            .order_by(models::Domain::VERIFIED_AT)
            .limit(100)
            .get(&*self.rt.db)
            .await?
            .into_models()
            .into_iter()
            .map(|row| row.host)
            .collect();
        let mut issued = Vec::new();
        for host in hosts {
            if self.issue_certificate(&host).await.is_ok() {
                issued.push(host);
            }
        }
        Ok(issued)
    }

    /// The certificate work that is due (what `domains:certificates --due`
    /// runs hourly), or, with `host`, that host's now:
    ///
    /// - `tls = "acme"`: issues the certificates verified domains and
    ///   `domains.acme.hosts` lack, and renews those within
    ///   `domains.acme.renew_days` of expiry;
    /// - `tls = "on_demand"` with `domains.certificates.probe`: reads each
    ///   verified domain's certificate over TLS for its expiry;
    /// - either way, reports certificates that expire within
    ///   `domains.certificates.warn_days`: a warning in the log, a
    ///   [`DomainCertificateExpiring`] event and, with the `lighthouse`
    ///   feature and navree-lighthouse in the app, an issue.
    ///
    /// A domain is due every `domains.certificates.check_hours`.
    pub async fn check_certificates(&self, host: Option<&str>) -> Result<CertificateRun, DomainsError> {
        let rt = self.rt.clone();
        let mut run = CertificateRun::default();
        if rt.options.tls == TlsMode::Off {
            return Ok(run);
        }
        let now = rt.now();
        let due_before = now - TimeDelta::hours(i64::from(rt.options.certificates.check_hours));
        let mut query = models::Domain::query()
            .where_(models::Domain::STATUS.eq(DomainStatus::Verified.as_str()))
            .order_by(models::Domain::HOST);
        if let Some(host) = host {
            let host = normalize_host(host).map_err(DomainsError::InvalidDomain)?;
            query = query.where_(models::Domain::HOST.eq(host));
        }
        let rows = query.get(&*rt.db).await?.into_models();
        for row in rows {
            let due = host.is_some()
                || row.tls_status == TlsStatus::Pending.as_str()
                || row.certificate_checked_at.is_none_or(|at| at <= due_before);
            if !due {
                continue;
            }
            match rt.options.tls {
                TlsMode::Acme => self.acme_due(&row, &mut run).await?,
                TlsMode::OnDemand => self.probe_due(&row, &mut run).await?,
                TlsMode::Off => {}
            }
        }
        if rt.options.tls == TlsMode::Acme && host.is_none() {
            self.own_hosts_due(&mut run).await?;
        }
        for (host, expires_at) in run.expiring.clone() {
            let domain_id = models::Domain::query()
                .where_(models::Domain::HOST.eq(host.clone()))
                .first(&*rt.db)
                .await?
                .map(|row| row.id);
            report_expiring(&rt, &host, domain_id, expires_at).await;
        }
        Ok(run)
    }

    async fn acme_due(&self, row: &models::Domain, run: &mut CertificateRun) -> Result<(), DomainsError> {
        let rt = &self.rt;
        let now = rt.now();
        let stored = models::Certificate::query()
            .where_(models::Certificate::HOST.eq(row.host.clone()))
            .first(&*rt.db)
            .await?;
        let renew_after = |not_after: DateTime<Utc>| {
            not_after - TimeDelta::days(i64::from(rt.options.acme.renew_days))
        };
        match stored.as_ref().map(|cert| cert.not_after) {
            Some(not_after) if renew_after(not_after) > now => {
                models::Domain::query()
                    .where_(models::Domain::ID.eq(row.id))
                    .update(
                        &*rt.db,
                        [
                            models::Domain::CERTIFICATE_CHECKED_AT.set(Some(now)),
                            models::Domain::CERTIFICATE_EXPIRES_AT.set(Some(not_after)),
                        ],
                    )
                    .await?;
                self.note_expiry(&row.host, not_after, run);
            }
            existing => match self.issue_certificate(&row.host).await {
                Ok(_) if existing.is_some() => run.renewed.push(row.host.clone()),
                Ok(_) => run.issued.push(row.host.clone()),
                Err(error) => {
                    run.failed.push((row.host.clone(), error.to_string()));
                    if let Some(not_after) = existing {
                        self.note_expiry(&row.host, not_after, run);
                    }
                }
            },
        }
        Ok(())
    }

    async fn own_hosts_due(&self, run: &mut CertificateRun) -> Result<(), DomainsError> {
        let rt = &self.rt;
        let now = rt.now();
        let hosts: Vec<String> = rt
            .options
            .acme
            .hosts
            .iter()
            .filter_map(|host| normalize_host(host).ok())
            .collect();
        for host in hosts {
            let stored = models::Certificate::query()
                .where_(models::Certificate::HOST.eq(host.clone()))
                .first(&*rt.db)
                .await?
                .map(|cert| cert.not_after);
            let due = stored.is_none_or(|not_after| {
                not_after - TimeDelta::days(i64::from(rt.options.acme.renew_days)) <= now
            });
            if !due {
                continue;
            }
            match self.issue_certificate(&host).await {
                Ok(_) if stored.is_some() => run.renewed.push(host),
                Ok(_) => run.issued.push(host),
                Err(error) => {
                    run.failed.push((host.clone(), error.to_string()));
                    if let Some(not_after) = stored {
                        self.note_expiry(&host, not_after, run);
                    }
                }
            }
        }
        Ok(())
    }

    async fn probe_due(&self, row: &models::Domain, run: &mut CertificateRun) -> Result<(), DomainsError> {
        let rt = &self.rt;
        if !rt.options.certificates.probe {
            return Ok(());
        }
        let now = rt.now();
        match rt.probe.probe(&row.host).await {
            Ok(found) => {
                models::Domain::query()
                    .where_(models::Domain::ID.eq(row.id))
                    .update(
                        &*rt.db,
                        [
                            models::Domain::TLS_STATUS.set(TlsStatus::Active.as_str()),
                            models::Domain::TLS_ERROR.set(None::<String>),
                            models::Domain::CERTIFICATE_EXPIRES_AT.set(Some(found.not_after)),
                            models::Domain::CERTIFICATE_CHECKED_AT.set(Some(now)),
                            models::Domain::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                run.probed.push((row.host.clone(), found.not_after));
                self.note_expiry(&row.host, found.not_after, run);
            }
            Err(error) => {
                models::Domain::query()
                    .where_(models::Domain::ID.eq(row.id))
                    .update(
                        &*rt.db,
                        [
                            models::Domain::TLS_STATUS.set(TlsStatus::Failed.as_str()),
                            models::Domain::TLS_ERROR.set(Some(error.to_string())),
                            models::Domain::CERTIFICATE_CHECKED_AT.set(Some(now)),
                            models::Domain::UPDATED_AT.set(now),
                        ],
                    )
                    .await?;
                run.failed.push((row.host.clone(), error.to_string()));
            }
        }
        Ok(())
    }

    /// Adds `host` to the run's expiring list when it expires within
    /// `domains.certificates.warn_days`.
    fn note_expiry(&self, host: &str, not_after: DateTime<Utc>, run: &mut CertificateRun) {
        let warn = TimeDelta::days(i64::from(self.rt.options.certificates.warn_days));
        if not_after - self.rt.now() <= warn {
            run.expiring.push((host.to_owned(), not_after));
        }
    }

    /// The answer to HTTP-01 challenge `token`, while it is published.
    pub async fn challenge_answer(&self, token: &str) -> Result<Option<String>, DomainsError> {
        let row = models::Challenge::query()
            .where_(models::Challenge::TOKEN.eq(token))
            .first(&*self.rt.db)
            .await?;
        Ok(row
            .filter(|row| row.expires_at > self.rt.now())
            .map(|row| row.into_inner().key_authorization))
    }
}

/// Logs, emits and (with Lighthouse) reports a certificate close to
/// expiry.
async fn report_expiring(rt: &Runtime, host: &str, domain_id: Option<Uuid>, expires_at: DateTime<Utc>) {
    let days_left = (expires_at - rt.now()).num_days().max(0);
    tracing::warn!(target: "navree::domains", host, %expires_at, days_left, "a certificate expires soon");
    rt.emit(DomainCertificateExpiring {
        host: host.to_owned(),
        domain_id,
        expires_at,
        days_left,
    })
    .await;
    #[cfg(feature = "lighthouse")]
    if let Some(app) = rt.app.upgrade()
        && let Some(lighthouse) = app.try_get::<navree_lighthouse::Lighthouse>().await
    {
        lighthouse.capture(
            navree_lighthouse::Report::message(format!(
                "The certificate for {host} expires in {days_left} days"
            ))
            .level(navree_lighthouse::Level::Warning)
            .tag("domain", host)
            .tag("expires_at", expires_at.to_rfc3339())
            .fingerprint(["domains", "certificate_expiring", host]),
        );
    }
}

/// The module's tables as an [`AcmeStore`]: sealed accounts, and
/// challenge answers any server can serve.
struct Store(Arc<Runtime>);

impl AcmeStore for Store {
    fn account<'a>(&'a self, directory: &'a str) -> BoxFuture<'a, Result<Option<String>, DomainsError>> {
        Box::pin(async move {
            let row = models::AcmeAccount::query()
                .where_(models::AcmeAccount::DIRECTORY_URL.eq(directory))
                .first(&*self.0.db)
                .await?;
            row.map(|row| self.0.open(&row.credentials)).transpose()
        })
    }

    fn save_account<'a>(
        &'a self,
        directory: &'a str,
        credentials: &'a str,
    ) -> BoxFuture<'a, Result<(), DomainsError>> {
        Box::pin(async move {
            let sealed = self.0.seal(credentials)?;
            let now = self.0.now();
            let existing = models::AcmeAccount::query()
                .where_(models::AcmeAccount::DIRECTORY_URL.eq(directory))
                .first(&*self.0.db)
                .await?;
            let mut record = existing.unwrap_or_else(|| {
                Record::new(models::AcmeAccount {
                    id: Uuid::now_v7(),
                    directory_url: directory.to_owned(),
                    created_at: now,
                    ..models::AcmeAccount::default()
                })
            });
            record.credentials = sealed;
            record.updated_at = now;
            record.save(&*self.0.db).await?;
            Ok(())
        })
    }

    fn publish<'a>(
        &'a self,
        host: &'a str,
        token: &'a str,
        key_authorization: &'a str,
    ) -> BoxFuture<'a, Result<(), DomainsError>> {
        Box::pin(async move {
            let now = self.0.now();
            models::Challenge::query()
                .where_(models::Challenge::EXPIRES_AT.lt(now))
                .delete(&*self.0.db)
                .await?;
            models::Challenge::query()
                .where_(models::Challenge::TOKEN.eq(token))
                .delete(&*self.0.db)
                .await?;
            Record::new(models::Challenge {
                id: Uuid::now_v7(),
                token: token.to_owned(),
                host: host.to_owned(),
                key_authorization: key_authorization.to_owned(),
                expires_at: now + TimeDelta::minutes(CHALLENGE_TTL_MINUTES),
                created_at: now,
            })
            .save(&*self.0.db)
            .await?;
            Ok(())
        })
    }

    fn withdraw<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<(), DomainsError>> {
        Box::pin(async move {
            models::Challenge::query()
                .where_(models::Challenge::TOKEN.eq(token))
                .delete(&*self.0.db)
                .await?;
            Ok(())
        })
    }
}
