# syntax=docker/dockerfile:1
# check=error=true;skip=SecretsUsedInArgOrEnv

# The production Dockerfile that `rails new` (8.1) generates, adapted for the
# benchmark (see README.md): Debian bookworm, Puma serving :8080 directly (no
# Thruster), no `db:prepare` at boot, BuildKit cache mounts for apt and gems.

# Make sure RUBY_VERSION matches the Ruby version in .ruby-version
ARG RUBY_VERSION=4.0.9
FROM docker.io/library/ruby:$RUBY_VERSION-slim-bookworm AS base

# Rails app lives here
WORKDIR /rails

# Keep downloaded .debs for the apt cache mounts below.
RUN rm -f /etc/apt/apt.conf.d/docker-clean

# Install base packages
RUN --mount=type=cache,id=bench-rails-apt-cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,id=bench-rails-apt-lib,target=/var/lib/apt,sharing=locked \
    apt-get update -qq && \
    apt-get install --no-install-recommends -y curl libjemalloc2 postgresql-client && \
    ln -s /usr/lib/$(uname -m)-linux-gnu/libjemalloc.so.2 /usr/local/lib/libjemalloc.so

# Set production environment variables and enable jemalloc for reduced memory usage and latency.
ENV RAILS_ENV="production" \
    BUNDLE_DEPLOYMENT="1" \
    BUNDLE_PATH="/usr/local/bundle" \
    BUNDLE_WITHOUT="development:test" \
    LD_PRELOAD="/usr/local/lib/libjemalloc.so"

# Throw-away build stage to reduce size of final image
FROM base AS build

# Install packages needed to build gems
RUN --mount=type=cache,id=bench-rails-apt-cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,id=bench-rails-apt-lib,target=/var/lib/apt,sharing=locked \
    apt-get update -qq && \
    apt-get install --no-install-recommends -y build-essential git libpq-dev libyaml-dev pkg-config

# Install application gems (downloaded .gem files are kept in a cache mount)
COPY Gemfile Gemfile.lock ./

RUN --mount=type=cache,id=bench-rails-gems,target=/bundle-cache \
    BUNDLE_USER_CACHE=/bundle-cache BUNDLE_GLOBAL_GEM_CACHE=true bundle install && \
    rm -rf ~/.bundle/ "${BUNDLE_PATH}"/ruby/*/cache "${BUNDLE_PATH}"/ruby/*/bundler/gems/*/.git && \
    # -j 1 disable parallel compilation to avoid a QEMU bug: https://github.com/rails/bootsnap/issues/495
    bundle exec bootsnap precompile -j 1 --gemfile

# Copy application code
COPY . .

# Precompile bootsnap code for faster boot times.
# -j 1 disable parallel compilation to avoid a QEMU bug: https://github.com/rails/bootsnap/issues/495
RUN bundle exec bootsnap precompile -j 1 app/

# Final stage for app image
FROM base

# Puma: one worker process per CPU (the benchmark gives the app 8), 5 threads
# each; Active Record's pool follows RAILS_MAX_THREADS (8 x 5 = 40 connections).
# Log level warn: no per-request logging.
# Rails 8.1 refuses to boot in production without secret_key_base. This API has
# no sessions, cookies or signed messages, so a placeholder is enough here; a
# real deployment injects SECRET_KEY_BASE (or RAILS_MASTER_KEY) at run time.
ENV PORT="8080" \
    WEB_CONCURRENCY="8" \
    RAILS_MAX_THREADS="5" \
    RAILS_LOG_LEVEL="warn" \
    SECRET_KEY_BASE="placeholder-the-benchmark-api-signs-and-encrypts-nothing"

# Run and own only the runtime files as a non-root user for security
RUN groupadd --system --gid 1000 rails && \
    useradd rails --uid 1000 --gid 1000 --create-home --shell /bin/bash
USER 1000:1000

# Copy built artifacts: gems, application
COPY --chown=rails:rails --from=build "${BUNDLE_PATH}" "${BUNDLE_PATH}"
COPY --chown=rails:rails --from=build /rails /rails

# Start the server by default, this can be overwritten at runtime
EXPOSE 8080
CMD ["./bin/rails", "server"]
