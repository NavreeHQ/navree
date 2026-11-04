#!/usr/bin/env bash
# One-time root setup for the live theme demo on navree.com (apps/theme-demo).
# Run it yourself, once, after theme-demo.sh has started the app as forge:
#
#   sudo bash /home/forge/navree.com/repo/deploy/navree.com/demo-ui-setup.sh
#
# What needs sudo, and all this does:
#   1. installs the request limit in nginx's http block
#      (/etc/nginx/conf.d/navree.com-demo-ui-limits.conf);
#   2. installs the locations /demo/ui/ and /_navree/ui/
#      (/etc/nginx/snippets/navree.com-demo-ui.conf);
#   3. installs this repository's nginx.conf for navree.com, whose server block
#      includes that snippet;
#   4. tests nginx and reloads it, or puts every file back if the test fails.
#
# Building the app and running it under pm2 needs no sudo (theme-demo.sh). The
# app listens on 127.0.0.1:8095 only. It touches no other site. Re-running it is
# safe. To take the demo off the site, remove the two installed files, test and
# reload nginx: the docs then show the builder's picture.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

if [ "$(id -u)" -ne 0 ]; then
  echo "Run with sudo." >&2
  exit 1
fi
if ! grep -q 'include /etc/nginx/conf.d/\*.conf' /etc/nginx/nginx.conf; then
  echo "/etc/nginx/nginx.conf doesn't include conf.d/*.conf in its http block: add" >&2
  echo "demo-ui-limits.conf to the http block yourself, then run this again." >&2
  exit 1
fi
if ! curl -fsS -o /dev/null http://127.0.0.1:8095/demo/ui/health; then
  echo "The theme demo isn't answering on 127.0.0.1:8095: run theme-demo.sh first." >&2
  exit 1
fi

limits=/etc/nginx/conf.d/navree.com-demo-ui-limits.conf
snippet=/etc/nginx/snippets/navree.com-demo-ui.conf
site=/etc/nginx/sites-available/navree.com
backup=$(mktemp -d)
cp -a "$site" "$backup/site"

install -d -m 755 /etc/nginx/snippets
install -m 644 "$here/demo-ui-limits.conf" "$limits"
install -m 644 "$here/demo-ui.conf" "$snippet"
install -m 644 "$here/nginx.conf" "$site"

if nginx -t; then
  systemctl reload nginx
  echo "navree.com now passes /demo/ui/ and /_navree/ui/ to the theme demo."
else
  echo "nginx -t failed: putting everything back." >&2
  rm -f "$limits" "$snippet"
  cp -a "$backup/site" "$site"
  nginx -t && systemctl reload nginx
  exit 1
fi
