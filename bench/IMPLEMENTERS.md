# Brief for benchmark implementers

You implement one framework's version of the benchmark API. Several agents
do this at the same time, each on a different framework, on one machine and
one Docker engine.

## Read first
- `bench/SPEC.md` is the contract. Follow it exactly, including the fairness rules in §4.
- `bench/frameworks/express/` is a finished reference implementation: Dockerfile, code and README.

## Rules
1. **Idiomatic production setup.** Build what a competent team would deploy on that framework: its recommended production server and worker model, its ORM, validation and auth integration (for full-stack frameworks), production config, and no request logging. No benchmark-only tricks: no response caching, no hand-rolled HTTP parsing, no disabling of the framework's normal request pipeline.
2. **Versions.** Use the latest stable releases of the framework, runtime and libraries; check the package registry, not your memory. Pin them with lockfiles. If a runtime isn't installed on the host (Go, PHP and Ruby aren't), generate lockfiles inside a container, for example `docker run --rm -v "<abs dir>:/app" -w /app composer:2 composer update`.
3. **Docker.**
   - Put the Dockerfile at `bench/frameworks/<dir>/` (see `frameworks.json` for each app's context and Dockerfile path).
   - Use Debian/glibc images and multi-stage builds for compiled code.
   - The image must start the server on 0.0.0.0:8080, ready within 60 s.
   - Read the database and JWT settings from the environment (SPEC §5).
   - Use BuildKit cache mounts (`RUN --mount=type=cache,...`) for package caches and, for Rust, the target dir, so rebuilds are fast.
4. **Memory is shared.** The Docker VM has 16 GB for every agent's builds plus PostgreSQL.
   - Rust builds: `cargo build --release --jobs 6`.
   - JVM/.NET builds: cap the build tool's heap where it's easy.
   - Don't start extra containers you don't need.
5. **Verify** from the repo root: `node bench/run.mjs --smoke --only <name>`. This:
   - builds the image;
   - copies the seeded database to your own `bench_<name>`;
   - starts your container;
   - runs `bench/tools/conformance.mjs`, which must print `PASS`;
   - runs a 5 s load test on each endpoint.

   Fix things until conformance passes and the load tests report no errors. A few timeouts on `json` while the app warms up are acceptable. Logs are in `bench/results/smoke/logs/<name>.log` and `<name>.build.log`. To debug, start your container yourself (name it `bench-dev-<name>`, network `bench`, env as in `run.mjs`'s `startApp`) and remove it when you're done. You can also run the conformance script directly against it, on a fresh database.
6. **Don't touch shared things.**
   - Don't edit `bench/SPEC.md`, `bench/run.mjs`, `bench/tools/*`, `bench/wrk/*`, `bench/db/*`, `bench/frameworks.json`, `bench/IMPLEMENTERS.md` or other frameworks' directories.
   - Don't stop or remove `bench-pg`, the `bench` network, the `bench-wrk` image or anyone else's containers.
   - Never run `docker system prune` or any other global cleanup.

   If a shared file looks wrong, or the spec is ambiguous, work around it and report it in your final answer.
7. **No performance runs.** Don't run `run.mjs` without `--smoke`; the real benchmark runs later, alone on the machine. Smoke numbers are noisy because other agents are building, so don't tune against them.
8. **Process.** Don't use SendMessage to contact other agents. Don't commit to git. Stay inside your own `bench/frameworks/<dir>/` (the Navis implementer has an extra exception, given in its own task).
9. **README.** Write `bench/frameworks/<dir>/README.md` like the Express one: versions, server and worker setup, DB library and pool size, JWT and validation libraries, and notes on any judgement calls.

## Final answer
For each app:
- conformance result and the smoke req/s per test;
- the exact versions used;
- anything that was ambiguous in the spec or broken in the harness;
- any choice you weren't sure was idiomatic.
