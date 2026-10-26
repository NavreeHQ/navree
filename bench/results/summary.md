# Benchmark results

Requests per second (median of 3 measured runs), higher is better. `wrk` 6 threads, 256 connections; the app limited to CPUs 0-7 and 4 GB, PostgreSQL CPUs 8-17, wrk CPUs 18-23.

| Framework | Language | JSON | Single query | 20 queries | Auth + paginated list | Auth + validated insert | Startup (ms) | Idle mem (MB) | Peak mem (MB) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Navree | Rust | 214,257 | 46,547 | 6,126 | 15,458 | 20,295 | 23 | 4 | 38 |
| Loco | Rust | 541,744 | 76,058 | 6,433 | 16,202 | 34,308 | 3 | 5 | 42 |
| Axum + sqlx | Rust | 962,842 | 107,994 | 7,103 | 24,402 | 49,043 | 3 | 3 | 31 |
| Actix Web + sqlx | Rust | 1,233,097 | 78,164 | 6,371 | 22,074 | 44,515 | 3 | 4 | 24 |
| Gin + pgx | Go | 524,631 | 184,935 | 21,856 | 29,964 | 71,747 | 2 | 5 | 49 |
| ASP.NET Core + EF Core | C# | 496,924 | 77,474 | 6,317 | 20,352 | 29,245 | 513 | 35 | 208 |
| Spring Boot + JPA | Java | 342,429 | 73,965 | 17,460 | 16,256 | 28,540 | 1,305 | 270 | 1,037 |
| NestJS (Express) | TypeScript | 356,528 | 64,988 | 6,036 | 4,532 | 13,790 | 1,365 | 632 | 2,699 |
| NestJS (Fastify) | TypeScript | 644,280 | 80,132 | 6,653 | 4,729 | 14,817 | 876 | 710 | 2,703 |
| Express + pg | JavaScript | 441,593 | 103,610 | 13,359 | 20,135 | 31,318 | 342 | 265 | 1,437 |
| FastAPI + SQLAlchemy | Python | 119,904 | 10,823 | 1,369 | 3,473 | 4,964 | 1,573 | 606 | 635 |
| Django + DRF | Python | 25,489 | 7,535 | 1,471 | 2,236 | 3,437 | 859 | 653 | 1,019 |
| Laravel (Octane, FrankenPHP) | PHP | 14,078 | 7,493 | 1,063 | 1,757 | 3,216 | 897 | 153 | 185 |
| Laravel (PHP-FPM + nginx) | PHP | 5,623 | 1,575 | 928 | 927 | 1,095 | 275 | 80 | 239 |
| Ruby on Rails | Ruby | 41,516 | 16,878 | 3,137 | 3,286 | 4,927 | 1,369 | 160 | 802 |

## p99 latency (ms)

| Framework | JSON | Single query | 20 queries | Auth + paginated list | Auth + validated insert |
|---|---:|---:|---:|---:|---:|
| Navree | 2.6 | 8.3 | 59.9 | 22.3 | 18.9 |
| Loco | 1.1 | 5.2 | 55.2 | 21.4 | 10.4 |
| Axum + sqlx | 0.6 | 4.0 | 52.3 | 14.8 | 8.2 |
| Actix Web + sqlx | 0.8 | 5.5 | 55.4 | 15.5 | 9.6 |
| Gin + pgx | 4.2 | 3.1 | 17.4 | 12.5 | 6.7 |
| ASP.NET Core + EF Core | 3.4 | 6.8 | 60.5 | 19.9 | 14.7 |
| Spring Boot + JPA | 5.6 | 18.8 | 46.4 | 65.4 | 29.6 |
| NestJS (Express) | 2.2 | 11.2 | 74.0 | 129.1 | 32.8 |
| NestJS (Fastify) | 1.3 | 12.1 | 79.4 | 108.4 | 31.1 |
| Express + pg | 2.0 | 5.2 | 30.2 | 28.8 | 18.1 |
| FastAPI + SQLAlchemy | 4.7 | 139.7 | 502.4 | 605.1 | 705.8 |
| Django + DRF | 72.6 | 153.4 | 696.8 | 456.9 | 261.0 |
| Laravel (Octane, FrankenPHP) | 28.4 | 49.8 | 281.1 | 175.9 | 115.4 |
| Laravel (PHP-FPM + nginx) | 62.0 | 199.9 | 321.1 | 314.8 | 281.9 |
| Ruby on Rails | 17.5 | 25.7 | 107.2 | 98.6 | 67.6 |
