# The navree facade: modules, crates and sections

An app depends on the `navree` crate alone (plus `serde` for its derives,
and `chrono` and `uuid`). Its default `full`
feature turns every module on; each module is also its own feature.
`postgres` adds the PostgreSQL driver, `mysql` the MySQL and MariaDB
driver and `sqlite` the SQLite driver (the bundled library). None is on by
default: a PostgreSQL or MySQL build compiles no SQLite, and tests, which
run on SQLite, turn `sqlite` on in `[dev-dependencies]`. `mongo` (opt-in) adds
MongoDB.

| Path | Crate | Feature | Module to add | Config section | What it gives |
|---|---|---|---|---|---|
| `navree::openapi` | navree-openapi | always | (the document model) | | `ToSchema` (trait and derive, also in the prelude), `Schema`, `Document` |
| `navree::kernel` | navree-kernel | always | (the kernel) | | `Navree::app()`, `AppBuilder`, `Module`, `ModuleBuilder`, the DI container, `CommandSpec` |
| `navree::bounded` | navree-bounded | always | | | the bounded in-process `Cache`: strict capacity, SIEVE eviction, TTL and TTI, single-flight loading |
| `navree::glob` | navree-glob | always | | | `Glob::new("/webhooks/*").matches(path)`: `*` any run of characters, `?` one, everything else literal and case-sensitive |
| `navree::contracts` | navree-contracts | always | | | `Principal`, `Job`, `JobContext`, `Clock`, `SystemClock`, `FixedClock`, `Broadcaster`, `Dispatcher`, `Secret<T>`, `RequestId`, `CredentialRevoker` |
| `navree::config` | navree-config | always | `ConfigModule::for_root(sources)` | | `ConfigSources`, `Config`, `ConfigSection` (`.env`, TOML and JSON files, `APP_*` variables) |
| `navree::telemetry` | navree-telemetry | `telemetry` (turned on by the modules that emit events) | | | typed telemetry events, sinks, W3C trace context |
| `navree::http` | navree-http | `http` | `HttpModule::from_config()` | `http` | controllers, `AppError`, `Path`, `Query`, `Json`, `Form`, `Extension`, `Created`, `NoContent`, `CurrentPrincipal`, `ClientIp`, `Guard`, `RouteDef` |
| `navree::log` | navree-log | `log` | `LoggerModule::from_config()` | `log` | tracing subscriber, JSON or pretty output |
| `navree::crypto` | navree-crypto | `crypto` | `CryptoModule::from_config()` | `hashing` (+ `app.key`) | AES-256-GCM `Encrypter`, Argon2id hasher, `generate_key`, `random_token`, `sha256_hex`; `jose`: `SigningKey`, `VerifyingKey`, `Jws`, `Jwk`, `JwkSet`; `totp`: `Totp`, `Secret` (RFC 6238, `otpauth://` URLs); `qr`: `QrCode`, `EcLevel` (ISO/IEC 18004, SVG) |
| `navree::events` | navree-events | `events` | `EventsModule::for_root()` | | `Event`, `Listener`, `EventBus` |
| `navree::view` | navree-view | `view` | `ViewModule::from_config()` | `view` | Templates (the Jinja syntax on navree-template), `Views` |
| `navree::cache` | navree-cache | `cache` | `CacheModule::from_config()` | `cache` | `Cache`, `Locks` |
| `navree::http_client` | navree-http-client | `http-client` | `HttpClientModule::from_config()` | `http_client` | `HttpClient`, `HttpClient::fake()` |
| `navree::schema` | navree-schema | `schema` | (used by migrations) | `schema` | `Schema`, `Blueprint`, `Migration`, `migration!`, `Target`, `Dialect` |
| `navree::db` | navree-db | `db` | `DatabaseModule::from_config()` | `database` | `Db`, `Uow`, `Executor`, `Statement`, `MigrationSet`, `Migrator`, `Seeder`, `Page`, `PageQuery` |
| `navree::orm` | navree-orm | `orm` | `OrmModule::from_config()` | `orm` | `#[derive(Model)]`, `Record`, `Collection`, `Builder`, `#[relations]`, `#[scopes]`, `binding::Bound`, `factory::ModelFactory`, `testing` |
| `navree::mongo` | navree-mongo | `mongo` (opt-in, not in `full`) | `MongoModule::from_config()` | `database.mongodb` | `MongoDb`, models with `driver = "mongodb"`, collections and their migrations |
| `navree::testing` | navree-testing | `testing` | | | `TestApp`, `TestClient`, `principal`, `temp_sqlite_url` |
| `navree::observe` | navree-observe | `observe` | `ObserveModule::from_config()` | `observe` | `/health/live`, `/health/ready`, `/metrics` |
| `navree::api` | navree-api | `api` | `ApiModule::from_config()` | `api` | `/api/openapi.json`, `/api/docs`, `openapi:export` |
| `navree::mail` | navree-mail | `mail` | `MailModule::from_config()` (needs `ViewModule`; the delivery pipeline also `DatabaseModule` and `CryptoModule`) | `mail` | `Mailer`, `TemplateMail`, `Mailable`, `Message`, `TransportResolver`, `Deliveries`, `Suppressions`, `Outbox`, `SendingDomains`, `MailFake` |
| `navree::realtime` | navree-realtime | `realtime` | `RealtimeModule::from_config()` | `realtime` | Pusher-protocol WebSockets, channels, `Realtime` |
| `navree::storage` | navree-storage | `storage` (+ `storage-s3`, `storage-gcs`, `storage-azure`) | `StorageModule::from_config()` | `storage` | `Disk`, `Storage`, `store_upload`, `presigned_put` / `accept_upload`, temporary URLs, `storage:sync` |
| `navree::session` | navree-session | `session` | `SessionModule::from_config()` | `session` | cookie sessions and CSRF (optional; not used with bearer tokens) |
| `navree::validation` | navree-validation | `validation` | `ValidationModule::from_config()` | `validation` | `Validate` (trait and derive), `Valid`, `ValidAsync`, `AsyncRule`, `ValidationErrors`, `Folded`, `Country`, `Locale`; `Phone` (`validation-phone`) and `SafeHtml` (`validation-html`) |
| `navree::queue` | navree-queue | `queue` | `QueueModule::from_config()` | `queue` | `Queue`, `Batch`, `Operations`, `QueueFake`, `queue:work` |
| `navree::audit` | navree-audit | `audit` | `AuditModule::from_config()` | `audit` | the audit trail, `GET /api/audit`, `audit:prune` |
| `navree::access` | navree-access | `access` | `AccessModule::for_root(AccessOptions { .. })` | | roles, permissions, abilities, `#[roles]`/`#[permissions]`/`#[can]` |
| `navree::schedule` | navree-schedule | `schedule` | `ScheduleModule::for_root()` | | `m.schedule`, `schedule:work`, `schedule:run` |
| `navree::notify` | navree-notify | `notify` | `NotifyModule::from_config()` | `notify` | `Notifier`, `Notification`, `/api/notifications` |
| `navree::auth` | navree-auth | `auth` | `AuthModule::from_config()` | `auth` | `/api/auth/*`, `AuthService`, `CurrentUser`, strategies, two-factor, sessions, `#[recent_auth]`, impersonation, passwordless, passkeys, platform sign-in, token exchange |
| `navree::settings` | navree-settings | `settings` | `SettingsModule::from_config()` + `SettingsModule::for_feature::<T>()` | `settings` | `#[derive(Settings)]`, `Settings`, `Scope`, `Resolved<T>`, `Setting<T>`, `/api/settings/{key}`, `settings:*` commands, `SettingsFake` |
| `navree::billing` | navree-billing | `billing` (opt-in, not in `full`) | `BillingModule::from_config()` + `BillingModule::for_feature::<Workspace>()` | `billing` | `Billing`, `Billable`, `Subscribed`, `/api/billing/*`, `BillingFake`, `billing:*` commands |
| `navree::dock` | navree-dock | `dock` (opt-in, not in `full`) | (a tool, not a module) | `dock.toml` | `DockFile`, `Dock`, `app_env`: the library behind `navree dock` |

Also re-exported: the macros (`module`,
`injectable`, `controller`, `get`, `post`, `put`, `patch`, `delete`) and,
at the root, `BoxFuture`, `KernelError`, `ModuleId`, `ProviderKey` and
`Scope`.

## The root a new app gets

`navree new` writes this `src/app.rs` (the kernel orders modules by their
imports and dependencies, so a module never runs before what it uses):

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(LoggerModule::from_config())
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())
    .module(EventsModule::for_root())
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
    .module(QueueModule::from_config())
    .module(ScheduleModule::for_root())
    .module(OrmModule::from_config())
    .module(Housekeeping) // schedules `model:prune` daily at 03:00
    .module(ValidationModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions {
        abilities: abilities::define,
        super_admin_role: Some("admin".to_owned()),
    }))
    .module(ApiModule::from_config())
    .module(ObserveModule::from_config())
    .module(LighthouseModule::from_config())
    .module(LogbookModule::from_config())
    .module(modules::users::UsersModule)
// navree:modules
```

Add `AuditModule::from_config()`, `NotifyModule::from_config()`,
`RealtimeModule::from_config()`, `StorageModule::from_config()` or
`HttpClientModule::from_config()` above the feature modules when a feature
needs them. Configure each framework module once, in the root; feature
modules import it by type.
