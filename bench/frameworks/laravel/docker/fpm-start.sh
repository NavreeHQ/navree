#!/bin/bash
# laravel-fpm: cache config/routes/events, then PHP-FPM and nginx side by side.
set -euo pipefail

# At start rather than at build time: the config cache bakes in DB_* and
# JWT_* from this container's environment.
php artisan optimize --no-interaction

php-fpm --nodaemonize &
# One nginx worker per CPU (nproc honours the container's cpuset).
nginx -g "daemon off; worker_processes $(nproc);" &

# docker stop sends SIGQUIT (the php image's STOPSIGNAL): a graceful stop
# for both. Exit as soon as either one dies.
stop() {
    kill -QUIT $(jobs -p) 2>/dev/null || true
    wait
    exit 0
}
trap stop QUIT TERM INT
wait -n
