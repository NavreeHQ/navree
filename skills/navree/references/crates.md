# The navree facade: modules, crates and sections

An app depends on the `navree` crate alone (plus `serde`, `sea-orm`,
`sea-orm-migration` and `garde` for their derives). Its default `full`
feature turns every module on; each module is also its own feature.
`postgres` adds the PostgreSQL driver (SQLite is the default and is
compiled in).

| Path | Crate | Feature | Module to add | Config section | What it gives |
|---|---|---|---|---|---|
| `navree::kernel` | navree-kernel | always | (the kernel) | | `Navree::app()`, `AppBuilder`, `Module`, `ModuleBuilder`, the DI container, `CommandSpec` |
| `navree::contracts` | navree-contracts | always | | | `Principal`, `Job`, `JobContext`, `Clock`, `SystemClock`, `FixedClock`, `Broadcaster`, `Dispatcher`, `Secret<T>`, `RequestId` |
| `navree::config` | navree-config | always | `ConfigModule::for_root(sources)` | | `ConfigSources`, `Config`, `ConfigSection` |
| `navree::http` | navree-http | `http` | `HttpModule::from_config()` | `http` | controllers, `AppError`, `Json`, `Created`, `NoContent`, `CurrentPrincipal`, `ClientIp`, `Guard`, `RouteDef` |
| `navree::log` | navree-log | `log` | `LoggerModule::from_config()` | `log` | tracing subscriber, JSON or pretty output |
| `navree::crypto` | navree-crypto | `crypto` | `CryptoModule::from_config()` | `hashing` (+ `app.key`) | AES-256-GCM `Encrypter`, Argon2id hasher, `generate_key`, `random_token`, `sha256_hex` |
| `navree::events` | navree-events | `events` | `EventsModule::for_root()` | | `Event`, `Listener`, `EventBus` |
| `navree::view` | navree-view | `view` | `ViewModule::from_config()` | `view` | MiniJinja templates, `Views` |
| `navree::cache` | navree-cache | `cache` | `CacheModule::from_config()` | `cache` | `Cache`, `Locks` |
| `navree::http_client` | navree-http-client | `http-client` | `HttpClientModule::from_config()` | `http_client` | `HttpClient`, `HttpClient::fake()` |
| `navree::db` | navree-db | `db` | `DatabaseModule::from_config()` | `database` | `Db`, `Uow`, `Repo`, `MigrationSet`, `Seeder`, `Factory`, `paginate`, `Page`, `PageQuery` |
| `navree::testing` | navree-testing | `testing` | | | `TestApp`, `TestClient`, `principal`, `temp_sqlite_url` |
| `navree::observe` | navree-observe | `observe` | `ObserveModule::from_config()` | `observe` | `/health/live`, `/health/ready`, `/metrics` |
| `navree::api` | navree-api | `api` | `ApiModule::from_config()` | `api` | `/api/openapi.json`, `/api/docs`, `openapi:export` |
| `navree::mail` | navree-mail | `mail` | `MailModule::from_config()` (needs `ViewModule`) | `mail` | `Mailer`, `TemplateMail`, `Mailable`, `Message` |
| `navree::realtime` | navree-realtime | `realtime` | `RealtimeModule::from_config()` | `realtime` | Pusher-protocol WebSockets, channels, `Realtime` |
| `navree::storage` | navree-storage | `storage` (+ `storage-s3`, `storage-gcs`, `storage-azure`) | `StorageModule::from_config()` | `storage` | `Disk`, `Storage`, `store_upload`, `presigned_put` / `accept_upload`, temporary URLs, `storage:sync` |
| `navree::session` | navree-session | `session` | `SessionModule::from_config()` | `session` | cookie sessions and CSRF (optional; not used with bearer tokens) |
| `navree::validation` | navree-validation | `validation` | `ValidationModule::from_config()` | `validation` | `Valid`, `ValidAsync`, `AsyncRule`, `ValidationErrors` |
| `navree::queue` | navree-queue | `queue` | `QueueModule::from_config()` | `queue` | `Queue`, `QueueFake`, `queue:work` |
| `navree::audit` | navree-audit | `audit` | `AuditModule::from_config()` | `audit` | the audit trail, `GET /api/audit`, `audit:prune` |
| `navree::access` | navree-access | `access` | `AccessModule::for_root(AccessOptions { .. })` | | roles, permissions, abilities, `#[roles]`/`#[permissions]`/`#[can]` |
| `navree::schedule` | navree-schedule | `schedule` | `ScheduleModule::for_root()` | | `m.schedule`, `schedule:work`, `schedule:run` |
| `navree::notify` | navree-notify | `notify` | `NotifyModule::from_config()` | `notify` | `Notifier`, `Notification`, `/api/notifications` |
| `navree::auth` | navree-auth | `auth` | `AuthModule::from_config()` | `auth` | `/api/auth/*`, `AuthService`, `CurrentUser`, strategies, two-factor |
| `navree::billing` | navree-billing | `billing` (opt-in, not in `full`) | `BillingModule::from_config()` + `BillingModule::for_feature::<Workspace>()` | `billing` | `Billing`, `Billable`, `Subscribed`, `/api/billing/*`, `BillingFake`, `billing:*` commands |
| `navree::dock` | navree-dock | `dock` (opt-in, not in `full`) | (a tool, not a module) | `dock.toml` | `DockFile`, `Dock`, `app_env`: the library behind `navree dock` |

Also re-exported: `navree::axum` (for `navree::axum::http::StatusCode`,
`navree::axum::extract::Multipart`, ...), the macros (`module`,
`injectable`, `controller`, `get`, `post`, `put`, `patch`, `delete`) and,
at the root, `BoxFuture`, `KernelError`, `ModuleId`, `ProviderKey` and
`Scope`.

## The root a new app gets

`navree new` writes this `src/app.rs` (the kernel orders modules by their
imports and dependencies, so a module never runs before what it uses):

```rust
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
    .module(ValidationModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions {
        abilities: abilities::define,
        super_admin_role: Some("admin".to_owned()),
    }))
    .module(ApiModule::from_config())
    .module(ObserveModule::from_config())
    .module(modules::users::UsersModule)
// navree:modules
```

Add `ScheduleModule::for_root()`, `AuditModule::from_config()`,
`NotifyModule::from_config()`, `RealtimeModule::from_config()`,
`StorageModule::from_config()` or `HttpClientModule::from_config()` above
the feature modules when a feature needs them. Configure each framework
module once, in the root; feature modules import it by type.
