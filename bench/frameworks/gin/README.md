# Gin (baseline for Go)

| | |
|---|---|
| Runtime | Go 1.27.3 (`golang:1.27-bookworm` to build, `debian:bookworm-slim` to run), a static binary (`CGO_ENABLED=0`, `-trimpath -s -w`) |
| Framework | Gin 1.12.0, `GIN_MODE=release`, `gin.New()` with `Recovery` only (no logger middleware), no trusted proxies |
| Server | Gin on `net/http` (`r.Run`), one process, a goroutine per connection. `GOMAXPROCS` is 8 under the benchmark's cpuset: the startup log line prints `GOMAXPROCS=8, NumCPU=8` |
| Database | pgx v5.11.0 with one `pgxpool`, `MaxConns` 64, plain SQL |
| JWT | golang-jwt/jwt v5.3.1 in a middleware: `WithValidMethods(HS256)`, `WithIssuer`, `WithExpirationRequired`, then the user row is loaded (401 when it is missing) |
| Validation | Gin's binding tags (go-playground/validator v10.30.1) on the query and body structs; failures give 400 with a JSON body |
| UUIDs | google/uuid 1.6.0 (v4 for new deals) |

Notes:
- `GOMAXPROCS`: Go sizes it from the process's CPU affinity mask, which `--cpuset-cpus` sets, and since Go 1.25 it also honours a cgroup CPU quota. Nothing is set by hand.
- pgx's default query mode caches prepared statements per connection, so PostgreSQL plans each query once per connection (unlike node-postgres in the Express baseline).
- `/queries` runs its N queries one after another on the pool, the usual Go style; a request holds at most one connection at a time.
- The deals list runs the count, then the page query, which loads owners with a `JOIN`.
- `amount_cents` binds to an `*int64` so `required` rejects a missing value but accepts `0`. A fractional number fails JSON decoding and gives 400.
- JSON uses Gin's default `encoding/json` (no `sonic`/`go_json` build tags).
- Errors from requests the client abandoned (`context.Canceled`, at the end of each wrk run) are not logged.
