# NestJS (Express and Fastify)

One codebase, two apps: `nestjs-express` (`Dockerfile.express`) and `nestjs-fastify` (`Dockerfile.fastify`). The Dockerfiles differ only in `HTTP_ADAPTER`, which `src/main.ts` reads to pick `ExpressAdapter` or `FastifyAdapter`.

| | |
|---|---|
| Runtime | Node.js 24 (`node:24-bookworm-slim`), `NODE_ENV=production` |
| Framework | NestJS 12.2 (`@nestjs/core`, `@nestjs/common`), ESM like the `nest new` template; built with `nest build` (CLI 12.0) and TypeScript 6.0, run from `dist/` |
| HTTP | `@nestjs/platform-express` 12.2 (Express 5.3, multer 2.5.0) or `@nestjs/platform-fastify` 12.2 (Fastify 5.12) |
| Workers | `node:cluster` in `main.ts`, one Nest app per CPU (`os.availableParallelism()`, 8 under the benchmark's cpuset) |
| Database | TypeORM 1.1 through `@nestjs/typeorm` 12.0, `pg` 8.23 driver; repositories injected into services; pool of `floor(64 / workers)` per worker (`extra: { max }`, 8 × 8 = 64) |
| JWT | `@nestjs/passport` 12.0 + `passport-jwt` 4.0 (jsonwebtoken 9): `JwtStrategy` with `ExtractJwt.fromAuthHeaderAsBearerToken()`, HS256, `issuer` checked, `AuthGuard('jwt')` on the deals controller |
| Validation | `class-validator` 0.15 + `class-transformer` 0.5, global `ValidationPipe({ transform: true })`, DTOs for the list query and the create body |
| Serialization | `ClassSerializerInterceptor` on the deals controller; entities use `@Exclude()`/`@Expose()` |
| Logging | Nest logger limited to `['error', 'warn']`, TypeORM logging off, no request logging |

Layout: `world/` (`/db`, `/queries`), `users/` (User entity + `UsersService`), `auth/` (`JwtStrategy`, `@CurrentUser()`), `deals/` (entity, DTOs, service, controller), and `app.controller.ts` (`/json`).

Notes:
- **Versions.** NestJS 12 is the npm `latest` (11.x is now tagged `legacy`). TypeScript is pinned to 6.0 rather than the npm `latest` 7.0 (the native Go port): the Nest 12 CLI depends on `typescript ~6.0.2`, and the schematics need TypeScript 6 as a peer.
- **multer override.** `@nestjs/platform-express` 12.2.0 (the latest release) pins `multer` to exactly 2.4.0, which has a DoS advisory (GHSA-q7qp-rjg7-q26m) fixed in 2.5.0. `package.json` has `"overrides": { "multer": "^2.5.0" }` until Nest ships a release with it. The fix is a patch release with the same dependencies, and the benchmark has no upload endpoints.
- **bigint.** `amount_cents` is a `bigint` column, which `pg` returns as a string; a TypeORM column transformer turns it into a number (amounts stay below 2^53).
- **Users and serialization.** `JwtStrategy.validate()` rejects a non-UUID `sub` before querying (`isUUID(sub, 'loose')`, because the seeded ids are md5 hashes, not RFC 9562 versions), then loads the full user row through `UsersService`. The User entity maps every column of Navree's `users` table; it is `@Exclude()`d at class level, so the API exposes only `id` and `name`.
- **List.** The deals list uses `repository.findAndCount({ relations: { owner: true }, order, skip, take })`. With a join plus `skip`/`take`, TypeORM paginates in two steps, then counts, so each request runs three queries in sequence on one connection:
  1. `SELECT DISTINCT` of the page's ids over the join;
  2. the page's deals `INNER JOIN users`, `WHERE id IN (...)`;
  3. `COUNT(DISTINCT id)` over the join.

  This is TypeORM's documented default. A QueryBuilder with `leftJoinAndSelect` + `offset`/`limit` would need one query fewer for this many-to-one join, but the TypeORM docs steer users to `skip`/`take`.
- **Create.** The provided schema has no column defaults, so the service sets the UUID (`crypto.randomUUID()`) and the timestamps itself. TypeORM's `uuid` generation and `@CreateDateColumn` would otherwise emit `DEFAULT`. It writes with `repository.insert()`, a single `INSERT`. `save()` would first `SELECT` the row by the preset id and wrap the insert in a transaction.
- **Queries and JSON bodies.**
  - `/queries` runs its N `findOneBy` calls concurrently on the pool (`Promise.all`), as the Express baseline does.
  - Malformed JSON gets the adapter's own 400: body-parser's error on Express, `FST_ERR_CTP_INVALID_JSON_BODY` on Fastify.
- **Config.** Configuration comes straight from `process.env`; `@nestjs/config` would only add a `.env` loader.
