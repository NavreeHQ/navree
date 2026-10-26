# Ruby on Rails (API-only)

| | |
|---|---|
| Runtime | Ruby 4.0.9 with YJIT (`ruby:4.0.9-slim-bookworm`), jemalloc 5.3 via `LD_PRELOAD` |
| Framework | Rails 8.1.4, `rails new --api` (Action Controller API, Active Record, Active Model only) |
| Server | Puma 8.0.2 in cluster mode: 8 workers (`WEB_CONCURRENCY=8`, one per CPU), 5 threads each (`RAILS_MAX_THREADS=5`), app preloaded and forked |
| Database | Active Record 8.1 + pg 1.7.0 (precompiled gem with its own libpq 18), prepared statements on; pool = threads = 5 per worker, 40 connections in total |
| JWT | jwt 3.3.0 (HS256, `exp` checked, `iss: "navree"` with `verify_iss: true`) in an `Authentication` controller concern; token parsed with Rails' `authenticate_with_http_token`, user loaded into `Current.user` |
| Validation | Active Record validations on `Deal` plus strong params (`params.expect`); an Active Model form object (`Pagination`) for `page`/`per_page` |
| Serialization | `render json:` with Active Model's `serializable_hash` (`only:` / `include: :owner`) |

Production settings: `RAILS_ENV=production`, eager loading, bootsnap (precompiled in the image), `RAILS_LOG_LEVEL=warn` (no request logging), `config.hosts` cleared, no migrations or `db:prepare` at boot. Lockfile generated in a `ruby:4.0.9-slim-bookworm` container (Bundler 4.0.20).

Layout:
- `app/models/{world,user,deal}.rb`: Active Record models on the existing tables. `World` sets `table_name = "world"`; `Deal belongs_to :owner, class_name: "User"`, `User has_many :deals` (with `inverse_of`, so a new deal gets its owner without a query). Deals get their UUID from `SecureRandom.uuid` in `before_create`, because `deals.id` has no database default.
- `app/controllers/concerns/authentication.rb`: the JWT `before_action`, 401 as JSON.
- `app/controllers/api/deals_controller.rb`: `Deal.count`, then `Deal.includes(:owner).order(created_at: :desc, id: :desc).limit(...).offset(...)` (one page query plus one `IN (...)` query for the owners). Create is `Current.user.deals.new(deal_params).save`.
- `app/controllers/worlds_controller.rb`: `/db` and `/queries` with `World.find`.

Notes and judgement calls:
- **No Thruster.** Rails 8's generated Dockerfile starts `./bin/thrust ./bin/rails server`. Thruster adds HTTP/2, Let's Encrypt TLS, gzip, X-Sendfile and asset caching for single-box Kamal deployments. A JSON API is more commonly deployed as Puma behind a load balancer or ingress that terminates TLS (Heroku, Render, ECS, Kubernetes, kamal-proxy). There, Thruster has nothing to do: no assets, no X-Sendfile, and wrk doesn't ask for gzip. It would only add a proxy hop on the app's 8 CPUs. So the app is generated with `--skip-thruster` and runs `./bin/rails server`, which is the generator's own non-Thruster command.
- The Dockerfile is the generated one with these changes: the bookworm base (pinned instead of the floating `-slim` tag), BuildKit cache mounts for apt and gem downloads, no `bin/docker-entrypoint` (its only job was `db:prepare`), and the env vars above. jemalloc stays enabled because the Rails 8 generator ships it (`LD_PRELOAD`).
- **SECRET_KEY_BASE.** Rails 8.1 won't boot in production without `secret_key_base`, so the Dockerfile sets a placeholder. The API has no sessions, cookies or signed messages. The generated credentials and master key were deleted, and a real deployment injects the secret at run time. The `SecretsUsedInArgOrEnv` build check is skipped for that line.
- `assume_ssl` / `force_ssl` stay as generated: the app assumes a TLS-terminating proxy. Requests aren't redirected; responses carry an HSTS header. Rails' default API middleware (request id, ETag, conditional GET, and so on) is untouched.
- `/queries` runs its N finds sequentially inside `World.uncached { ... }`. Otherwise Rails' per-request query cache would answer a repeated random id from memory instead of the database (SPEC: every request reads the database).
- **Validation statuses.** Invalid `page`/`per_page` and invalid deal bodies return 422 (`:unprocessable_content`) with Active Model error JSON. A malformed JSON body or a missing `deal` object returns a JSON 400 (`rescue_from` in `ApplicationController`).
- `page`/`per_page` are validated as raw strings (`numericality: { only_integer: true }`), so `abc` and `1.5` are rejected. `ActiveModel::Attributes` would type-cast them to 0 or 1 before validation.
- Pagination is plain Active Record `limit`/`offset` plus `count`. Kaminari or pagy would issue the same queries.
- Puma reads `WEB_CONCURRENCY` itself and preloads the app by default when it has more than one worker, so `config/puma.rb` is the generated one. `PORT=8080` makes `rails server` bind `tcp://0.0.0.0:8080` (Puma 8 would otherwise prefer `::` on IPv6 hosts).
