# ASP.NET Core MVC + EF Core

| | |
|---|---|
| Runtime | .NET 10.0.14 (LTS): `mcr.microsoft.com/dotnet/sdk:10.0` (SDK 10.0.403) to build, `mcr.microsoft.com/dotnet/aspnet:10.0` to run |
| Framework | ASP.NET Core 10 MVC controllers with `[ApiController]` and attribute routing |
| Server | Kestrel, one process, default thread pool; Server GC (the Web SDK default; DATAS is on by default since .NET 9); `ASPNETCORE_ENVIRONMENT` unset, so Production |
| Database | EF Core 10.0.14 with Npgsql.EntityFrameworkCore.PostgreSQL 10.0.3 (Npgsql 10.0.3). `AddDbContextPool`, LINQ queries, `AsNoTracking` for reads; owners come from a projection (one `JOIN`). `Maximum Pool Size=64` in the connection string, which is built from `DATABASE_URL` (or the `DB_*` variables) |
| JWT | Microsoft.AspNetCore.Authentication.JwtBearer 10.0.14 (Microsoft.IdentityModel 8.19.2): symmetric key, HS256 only (`ValidAlgorithms`), `ValidIssuer`, lifetime checked; `MapInboundClaims = false` so `sub` stays `sub` |
| Validation | DataAnnotations (`Required`, `Range`, `StringLength`, `AllowedValues`); `[ApiController]` returns the automatic 400 `ValidationProblemDetails` |
| Logging | Minimum level Warning (`appsettings.json`), no request logging |

Notes:
- **User loading.** `JwtBearerEvents.OnTokenValidated` loads the user by `sub` from the pooled `DbContext`. A missing user calls `context.Fail(...)`, so `[Authorize]` answers 401. The entity is stored as an `HttpContext` feature for the controller.
- **ProblemDetails content type.** `[ApiController]` sends validation errors as `application/problem+json`, but `tools/conformance.mjs` requires the content type to contain `application/json`. The default `InvalidModelStateResponseFactory` is wrapped so the same `ValidationProblemDetails` body goes out as `application/json`. Nothing else about validation changes.
- **JSON naming.** `JsonNamingPolicy.SnakeCaseLower` globally, so the API's snake_case keys (`amount_cents`, `per_page`, ...) map to C# names in both directions. `/db` overrides one property with `[JsonPropertyName("randomNumber")]`.
- **Inserts.** EF Core generates the new deal's id client-side (the Npgsql provider's UUIDv7 generator) and `SaveChangesAsync` inserts it.
- **`/queries`** awaits its N queries one after another, because a `DbContext` is not thread-safe.
- **Npgsql defaults.** Apart from the pool size, the connection string uses Npgsql's defaults: no automatic statement preparation (`Max Auto Prepare=0`) and reset on close. EF Core opens and closes the pooled connection around each query, so every checkout sends `DISCARD ALL` before the next statement. `No Reset On Close`, `Max Auto Prepare` and `Multiplexing` are tuning knobs a team might add, but they aren't the default, so they're left off. A quick A/B on `/db` during the smoke phase was too noisy to show a difference.
- **Base image.** Microsoft publishes .NET 10 images on Ubuntu 24.04 "noble", Alpine and Azure Linux only, with no Debian tag. The default `10.0` tags (Ubuntu, glibc) are used.
- **Build memory.** The build stage caps the SDK's GC heap with `DOTNET_GCHeapHardLimit=0x80000000` (2 GB).
- **Startup warnings.** Each start logs two one-time warnings. DataProtection warns that its key ring is ephemeral; authentication registers DataProtection, and the API doesn't use it. Npgsql can't load `libgssapi_krb5.so.2` for optional GSS encryption and falls back to a plain connection.
