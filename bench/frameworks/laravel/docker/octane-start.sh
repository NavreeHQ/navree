#!/bin/sh
# laravel-octane: cache config/routes/events, then Octane on FrankenPHP.
set -e

# At start rather than at build time: the config cache bakes in DB_* and
# JWT_* from this container's environment.
php artisan optimize --no-interaction

# One worker per CPU (nproc honours the container's cpuset: 8 in the benchmark).
exec php artisan octane:start \
    --server=frankenphp \
    --host=0.0.0.0 \
    --port=8080 \
    --workers="${OCTANE_WORKERS:-$(nproc)}"
