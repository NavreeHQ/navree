# The load generator: wrk (LuaJIT build) plus the benchmark's request scripts.
FROM debian:bookworm-slim AS build
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential git ca-certificates unzip perl \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch 4.2.0 https://github.com/wg/wrk.git /src/wrk
# wrk times requests and its own run with the wall clock (gettimeofday), and
# its event loop treats a backwards wall-clock step as clock skew and fires
# every timer at once. Docker Desktop's VM steps its wall clock now and then,
# which showed up as phantom "timeouts" (negative request durations) and
# would distort durations. Use the monotonic clock everywhere instead.
RUN cd /src/wrk/src \
 && perl -0pi -e 's/gettimeofday\(&(\w+), NULL\);/{ struct timespec mono_; clock_gettime(CLOCK_MONOTONIC, &mono_); $1.tv_sec = mono_.tv_sec; $1.tv_usec = mono_.tv_nsec \/ 1000; }/g' wrk.c ae.c \
 && perl -0pi -e 's/\btime\(NULL\)/({ struct timespec mono_; clock_gettime(CLOCK_MONOTONIC, &mono_); (time_t) mono_.tv_sec; })/g' ae.c \
 && ! grep -n "gettimeofday\|time(NULL)" wrk.c ae.c \
 && make -C /src/wrk -j"$(nproc)"

FROM debian:bookworm-slim
COPY --from=build /src/wrk/wrk /usr/local/bin/wrk
COPY scripts /scripts
# A benchmark harness image: built and run only on a benchmarking machine, never deployed.
# nosemgrep: dockerfile.security.missing-user-entrypoint.missing-user-entrypoint
ENTRYPOINT ["wrk"]
