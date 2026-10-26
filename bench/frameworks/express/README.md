# Express (baseline for Node.js)

| | |
|---|---|
| Runtime | Node.js 24 (`node:24-bookworm-slim`) |
| Framework | Express 5.3 |
| Workers | `node:cluster`, one worker per CPU (`os.availableParallelism()`, 8 under the benchmark's cpuset) |
| Database | node-postgres (`pg`) 8, a pool of 8 per worker (64 in total), plain SQL |
| JWT | jsonwebtoken 9 (HS256, issuer checked), then the user row is loaded |
| Validation | zod 4 |

Notes:
- `/queries` runs its N queries concurrently on the pool (`Promise.all`).
- The deals list runs the count and the page query concurrently.
- node-postgres sends unnamed statements, so PostgreSQL plans every query; drivers that cache prepared statements load the database less.
