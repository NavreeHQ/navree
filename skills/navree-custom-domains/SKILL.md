---
name: navree-custom-domains
description: "Custom domains with navree-domains: `DomainsModule` and the `domains` config section, `Domains::add` with `NewDomain` and the DNS records it returns (TXT at `_domain-verification.<host>`, CNAME to `domains.target`, A/AAAA for apex domains), verification (`verify`, `verify_due`, `domains:verify --due`, giving up, rechecks), the `domain` workspace resolver (`workspace.resolvers = [\"domain\", ...]`, `DomainWorkspace`) and the `CurrentDomain` extractor, automatic TLS (`tls = \"on_demand\"` with the ask endpoint `/_domains/allow` and Harbor's `[web.custom_domains]`, or `tls = \"acme\"` with `InstantAcme`, HTTP-01 answers, sealed keys, `DomainCertificates` and `HttpsServer`), managed Cloudflare zones, certificate expiry reports (`domains:certificates`, `DomainCertificateExpiring`, Lighthouse), the `/api/domains` routes, and tests with `DnsFake`, `AcmeFake`, `ProbeFake` and `DnsProviderFake`. Use when a workspace needs its own domain, when a domain stays pending, or when certificates for customers' domains fail."
---

# Custom domains

`DomainsModule` (crate navree-domains, facade feature `domains`, in `full`)
puts workspaces on their own domains: it hands out the DNS records to
create, verifies them on a schedule, gets certificates, and resolves the
workspace from the request's host. Everything below is in the code; when in
doubt read `crates/navree-domains/src` and the tests in
`crates/navree-domains/tests/domains/` (`adding.rs`, `verification.rs`,
`resolution.rs`, `routes.rs`, `certificates.rs` and `https.rs` for ACME
with the fake CA and a real TLS handshake, `probes.rs`, `managed.rs`,
`cloudflare.rs` on wiremock, `servers.rs` on PostgreSQL and MySQL,
`pebble.rs` against Let's Encrypt's test CA) and
`crates/navree/tests/domains_docs.rs` (the page's examples through the
facade). The page is `docs/packages/custom-domains`.

## Steps: let workspaces bring their own domain

1. Add the modules. Required: Config, Database. Workspace for the routes
   and the resolver; Schedule so `domains:verify --due` runs every minute;
   Events for the events; Crypto with `APP_KEY` for `tls = "acme"`;
   Access + an authenticator for the routes (`domains.routes = false`
   without them).

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(ScheduleModule::for_root())
    .module(AccessModule::for_root(access))
    .module(AuthModule::from_config())
    .module(WorkspaceModule::from_config())
    .module(DomainsModule::from_config())      // section `domains`
```

2. Configure where customers point their domains, and name the TXT record
   after the product (the default `_domain-verification` works too):

```toml
[domains]
target = "domains.example.com"     # CNAME target
addresses = ["203.0.113.10"]       # A records for apex domains

[domains.verification]
prefix = "_example-verification"

[workspace]
resolvers = ["domain", "header"]   # a verified domain names its workspace
```

3. Pick the TLS mode (`domains.tls`):
   - `on_demand` (default): a proxy asks `GET /_domains/allow?domain=`
     (200 only for verified domains) before it gets a certificate. With
     Harbor, add `[web.custom_domains] enabled = true` to `harbor.toml`
     and run `navree harbor:provision`.
   - `acme`: the app gets certificates itself. Set
     `domains.acme.terms_agreed = true` (boot error otherwise), `contact`,
     and `https_addr = "0.0.0.0:443"` so `serve` also listens for HTTPS;
     the plain server (`http.addr`, port 80) answers the HTTP-01
     challenges at `/.well-known/acme-challenge/{token}`. Try
     `directory_url = LETS_ENCRYPT_STAGING` first.
   - `off`: something else terminates TLS.

4. Build the settings page on the routes (permission `domains.manage`,
   `InWorkspace` guard; owners have it through `all.*`):
   `GET/POST /api/domains`, `GET/DELETE /api/domains/{id}`,
   `POST /api/domains/{id}/verify` ("Check now"),
   `POST /api/domains/{id}/primary`. Show `dns_records` (each
   `{type, name, value, purpose}`) and `last_error` while pending.

5. Public pages on a customer's domain take `CurrentDomain` (no sign-in;
   404 `domain_not_found` for other hosts; `Option<CurrentDomain>` for
   pages that work on both). Links: `domains.primary_url(workspace)`.

## How verification works

- `add` normalizes the host (`normalize_host`), refuses reserved hosts
  (`app.url`'s host, `workspace.base_domain`, `domains.target`,
  `domains.reserved`, and their subdomains: `domain_not_allowed`), taken
  ones (`domain_taken`) and full workspaces (`domain_limit_reached`,
  `domains.max_per_workspace`, default 10).
- `verify_due` checks due domains: the TXT record holds the token, and
  (with `target` or `addresses` set and `require_routing`, the default)
  the host has a CNAME to the target or an address of the target or of
  `addresses`. Pending domains retry every `interval_mins` (5) and fail
  after `give_up_after_hours` (72); `verify(id)` checks any status now.
  Verified domains are rechecked every `recheck_hours` (24) for routing
  only: they stay verified, with `points_to_app = false` and a warning.
- A resolver error is recorded in `last_error` and returned as 502
  `dns_lookup_failed`; it is not a missing record.
- `resolve(host)` (what the resolver layer and `CurrentDomain` use) is
  cached for `cache_secs` (60). Writes clear the entry on this server only.

## Certificates

- ACME: `domains:verify --due` calls `issue_pending()` after its checks;
  `domains:certificates --due` (hourly) renews within `renew_days` (30) and
  issues for `domains.acme.hosts`. Keys and the account are sealed with the
  app key; without one the issuance fails before ordering. Every server
  loads certificates from the database (`reload_certificates`, every
  `reload_secs`). For your own TLS server: `domains.certificates()
  .server_config()`.
- On-demand: `domains:certificates --due` probes each verified domain on
  443 (`TlsProbe`) for `certificate_expires_at`; `probe = false` stops it.
- Both: within `warn_days` (14): a warning, `DomainCertificateExpiring`,
  and a Lighthouse issue when the `lighthouse` feature is on and
  `LighthouseModule` is in the app.

## Managed zones

`domains.dns.provider = "cloudflare"`, `domains.dns.zone`, and
`domains.dns.cloudflare.{api_token, zone_id}`: hosts inside the zone get
their CNAME (or A/AAAA) through the API and are verified at once
(`managed: true`, no TXT); `remove` deletes the record. Your own provider:
`impl DnsProvider` and `override_value::<dyn DnsProvider>(..)`.

## Tests

```rust
let dns = DnsFake::new().with_address("domains.example.com", "203.0.113.10");
let ca = AcmeFake::new();
let app = TestApp::builder()
    .temp_database()
    .config("domains.target", "domains.example.com")
    // ...modules...
    .override_value::<dyn DnsLookup>(Arc::new(dns.clone()))
    .override_value::<dyn AcmeClient>(Arc::new(ca.clone()))
    .override_value::<dyn CertificateProbe>(Arc::new(ProbeFake::new()))
    .override_value::<dyn Clock>(clock.clone())   // walk retries, give-ups, renewals
    .build()
    .await;
// After `add`: dns.set_txt(&domain.dns_records[0].name, &domain.dns_records[0].value);
//              dns.set_cname(host, "domains.example.com");
```

- `DnsFake`: `with_txt/with_cname/with_address` builders, `set_*` after
  boot, `remove`, `fail(name, reason)`, `lookups()`.
- `AcmeFake`: certificates from its own root (`ca_pem()` to trust it),
  `expire_at`, `fail_next`, `keep_challenges` (to GET the answer from the
  app), `orders()`, `challenges()`.
- `ProbeFake`: `expires(host, at)`, `fail(host, reason)`.
- `DnsProviderFake`: `records(kind, name)`, `fail_next`.
- `HttpsServer::bind(app.ctx(), "127.0.0.1:0")` then `serve()` serves the
  app over TLS in a test.

## Gotchas

- The `domain` resolver needs `DomainsModule` and must come before
  `header` to win; `domain` in organization or partition resolvers fails
  the boot.
- A custom domain only resolves once verified; a pending domain gives 400
  "names no workspace" through `CurrentWorkspace`.
- Caddy's `on_demand_tls` is a server-wide option: one app per server
  serves custom domains.
- `domains.tls = "acme"` needs port 80 reachable for HTTP-01 and the app
  key to seal keys; renewals start 30 days before expiry, reports at 14.
