//! [`SealedTickets`]: navree-contracts' `CallTickets` on the application
//! key and the cache.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use navree_cache::Cache;
use navree_contracts::{
    BoxFuture, CallTickets, Clock, IssuedTicket, RedeemedTicket, TicketCheck, TicketError,
    TicketHolder, TicketRequest,
};
use navree_crypto::{AppKey, CryptoError, Encrypter, random_token, sha256_hex};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a sealed ticket says inside, as its version names it.
const VERSION: &str = "navree.call.1";

/// The purpose the tickets' key is derived for from the application key.
const KEY_PURPOSE: &str = "navree-auth.call-tickets";

/// The longest ticket accepted, in bytes (a sealed ticket is a few hundred).
const MAX_TICKET_LEN: usize = 4096;

/// The inside of a ticket. The session is kept as its SHA-256.
#[derive(Serialize, Deserialize)]
struct Sealed {
    v: String,
    /// A random value naming this ticket (its use is recorded under it).
    n: String,
    u: Option<Uuid>,
    s: Option<String>,
    f: String,
    m: String,
    p: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<String>,
    i: i64,
    e: i64,
}

/// Sealed one-time tickets (navree-contracts' `CallTickets`): AES-256-GCM
/// with a key of their own, derived from the application key for this one
/// purpose (navree-crypto's `Encrypter::derive_key`), so a client can
/// neither read nor make one, nothing else sealed with the application key
/// opens as a ticket, and single use through the cache (an atomic "add if
/// absent" under the ticket's random name until it expires).
///
/// `AuthModule` registers one as `dyn CallTickets`, on the app's cache when
/// `CacheModule` is in the app (shared by every instance with a shared
/// store), else an in-process one. Issuing stores nothing. With more than
/// one instance, a ticket is single use across them only on a shared cache.
pub struct SealedTickets {
    encrypter: Arc<Encrypter>,
    cache: Cache,
    clock: Arc<dyn Clock>,
}

impl SealedTickets {
    /// Tickets sealed with a key derived from `encrypter`'s current key for
    /// tickets alone, used once in `cache`, timed by `clock`. Without an
    /// application key, issuing fails (`TicketError::Unavailable`).
    pub fn new(encrypter: Arc<Encrypter>, cache: Cache, clock: Arc<dyn Clock>) -> Self {
        let encrypter = match encrypter.derive_key(KEY_PURPOSE) {
            Ok(key) => Arc::new(Encrypter::new(AppKey::from_bytes(key))),
            // No current key: `encrypter` seals nothing either, and the
            // error says so.
            Err(_) => encrypter,
        };
        Self {
            encrypter,
            cache,
            clock,
        }
    }

    fn open(&self, ticket: &str) -> Result<Sealed, TicketError> {
        if ticket.is_empty() || ticket.len() > MAX_TICKET_LEN {
            return Err(TicketError::Malformed);
        }
        let sealed: Sealed = self
            .encrypter
            .decrypt_json(ticket)
            .map_err(|_| TicketError::Malformed)?;
        if sealed.v != VERSION {
            return Err(TicketError::Malformed);
        }
        Ok(sealed)
    }

    async fn redeem_ticket(
        &self,
        ticket: &str,
        check: &TicketCheck,
    ) -> Result<RedeemedTicket, TicketError> {
        let sealed = self.open(ticket)?;
        let now = self.clock.now();
        if now.timestamp() >= sealed.e {
            return Err(TicketError::Expired);
        }
        if sealed.u != check.holder.user || sealed.s != session_hash(&check.holder) {
            return Err(TicketError::WrongHolder);
        }
        if sealed.f != check.feature {
            return Err(TicketError::WrongFeature);
        }
        if check.method.as_ref().is_some_and(|m| *m != sealed.m)
            || check.path.as_ref().is_some_and(|p| *p != sealed.p)
        {
            return Err(TicketError::WrongRoute);
        }
        let left = u64::try_from(sealed.e - now.timestamp()).unwrap_or(1);
        let first = self
            .cache
            .add(
                &format!("navree-auth:ticket:{}", sha256_hex(sealed.n.as_bytes())),
                &true,
                Duration::from_secs(left + 1),
            )
            .await
            .map_err(|error| TicketError::Unavailable(error.to_string()))?;
        if !first {
            return Err(TicketError::Used);
        }
        Ok(RedeemedTicket::new(
            sealed.f,
            sealed.m,
            sealed.p,
            sealed.b,
            at(sealed.i),
            at(sealed.e),
        ))
    }
}

/// The SHA-256 of the holder's session, if any.
fn session_hash(holder: &TicketHolder) -> Option<String> {
    holder
        .session
        .as_deref()
        .map(|session| sha256_hex(session.as_bytes()))
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap_or_default()
}

impl CallTickets for SealedTickets {
    fn issue(&self, request: &TicketRequest) -> Result<IssuedTicket, TicketError> {
        let now = self.clock.now();
        let ttl = request
            .ttl
            .clamp(Duration::from_secs(1), TicketRequest::MAX_TTL);
        let expires_at = now + TimeDelta::from_std(ttl).unwrap_or_else(|_| TimeDelta::seconds(300));
        let sealed = Sealed {
            v: VERSION.to_owned(),
            n: random_token(22),
            u: request.holder.user,
            s: session_hash(&request.holder),
            f: request.feature.clone(),
            m: request.method.clone(),
            p: request.path.clone(),
            b: request.binding.clone(),
            i: now.timestamp(),
            e: expires_at.timestamp(),
        };
        let ticket = self
            .encrypter
            .encrypt_json(&sealed)
            .map_err(|error| match error {
                CryptoError::MissingKey => {
                    TicketError::Unavailable("no application key (`app.key`) is set".to_owned())
                }
                other => TicketError::Unavailable(other.to_string()),
            })?;
        Ok(IssuedTicket::new(ticket, at(sealed.e)))
    }

    fn redeem<'a>(
        &'a self,
        ticket: &'a str,
        check: &'a TicketCheck,
    ) -> BoxFuture<'a, Result<RedeemedTicket, TicketError>> {
        Box::pin(async move {
            let result = self.redeem_ticket(ticket, check).await;
            if let Err(error) = &result {
                tracing::warn!(
                    target: "navree::security",
                    feature = %check.feature,
                    reason = error.code(),
                    "a call ticket was refused"
                );
            }
            result
        })
    }
}

impl fmt::Debug for SealedTickets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SealedTickets").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use navree_contracts::FixedClock;
    use navree_crypto::AppKey;

    use super::*;

    fn tickets() -> (SealedTickets, Arc<FixedClock>) {
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let tickets = SealedTickets::new(
            Arc::new(Encrypter::new(AppKey::generate())),
            Cache::memory(),
            clock.clone(),
        );
        (tickets, clock)
    }

    fn holder(session: &str) -> TicketHolder {
        TicketHolder {
            user: Some(Uuid::nil()),
            session: Some(session.to_owned()),
        }
    }

    #[tokio::test]
    async fn a_ticket_says_nothing_readable_and_works_once() {
        let (tickets, _) = tickets();
        let request = TicketRequest::new(holder("phone"), "scan_code", "POST", "/stock/scan")
            .binding("digest-1");
        let issued = tickets.issue(&request).unwrap();
        for secret in ["phone", "scan_code", "/stock/scan", "digest-1"] {
            assert!(!issued.ticket.contains(secret), "{secret}");
        }
        let check = TicketCheck::new(holder("phone"), "scan_code").at("POST", "/stock/scan");
        let redeemed = tickets.redeem(&issued.ticket, &check).await.unwrap();
        assert_eq!(redeemed.binding.as_deref(), Some("digest-1"));
        assert_eq!(redeemed.expires_at, issued.expires_at);
        assert_eq!(
            tickets.redeem(&issued.ticket, &check).await,
            Err(TicketError::Used)
        );
    }

    #[tokio::test]
    async fn another_keys_ticket_is_malformed() {
        let (tickets, _) = tickets();
        let (other, _) = self::tickets();
        let request = TicketRequest::new(TicketHolder::guest(), "share", "POST", "/s");
        let issued = other.issue(&request).unwrap();
        let check = TicketCheck::new(TicketHolder::guest(), "share");
        assert_eq!(
            tickets.redeem(&issued.ticket, &check).await,
            Err(TicketError::Malformed)
        );
        // Another sealed value of the same key is not a ticket either.
        let sealed = tickets
            .encrypter
            .encrypt_json(&serde_json::json!({ "v": "x" }));
        assert_eq!(
            tickets.redeem(&sealed.unwrap(), &check).await,
            Err(TicketError::Malformed)
        );
        assert_eq!(
            tickets.redeem(&"a".repeat(5000), &check).await,
            Err(TicketError::Malformed)
        );
    }

    #[tokio::test]
    async fn tickets_are_sealed_with_a_key_of_their_own() {
        let general = Arc::new(Encrypter::new(AppKey::generate()));
        let clock = Arc::new(FixedClock::new(Utc::now()));
        let tickets = SealedTickets::new(general.clone(), Cache::memory(), clock.clone());
        let check = TicketCheck::new(holder("phone"), "scan_code").at("POST", "/stock/scan");

        // The application key opens no ticket.
        let issued = tickets
            .issue(&TicketRequest::new(
                holder("phone"),
                "scan_code",
                "POST",
                "/stock/scan",
            ))
            .unwrap();
        assert!(general.decrypt(&issued.ticket).is_err());

        // Nor does anything it seals open as a ticket, even a ticket's inside.
        let now = clock.now().timestamp();
        let inside = Sealed {
            v: VERSION.to_owned(),
            n: random_token(22),
            u: Some(Uuid::nil()),
            s: session_hash(&holder("phone")),
            f: "scan_code".to_owned(),
            m: "POST".to_owned(),
            p: "/stock/scan".to_owned(),
            b: None,
            i: now,
            e: now + 60,
        };
        let sealed = general.encrypt_json(&inside).unwrap();
        assert_eq!(
            tickets.redeem(&sealed, &check).await,
            Err(TicketError::Malformed)
        );

        // The same application key makes the same ticket key: another
        // instance redeems what this one issued.
        let other = SealedTickets::new(general, Cache::memory(), clock);
        assert!(other.redeem(&issued.ticket, &check).await.is_ok());
    }
}
