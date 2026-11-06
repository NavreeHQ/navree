#!/usr/bin/env bash
# One-time root setup for navree.com on the production server. Run it once,
# yourself, after the first deploy has created /home/forge/navree.com/current:
#
#   sudo bash /home/forge/navree.com/repo/deploy/navree.com/root-setup.sh
#
# It does exactly two things, the same way the server's other sites are set up:
#   1. a Let's Encrypt certificate for navree.com and www.navree.com through
#      certbot's Cloudflare DNS plugin (the method every other site here uses);
#   2. the nginx site from nginx.conf (and the security headers file it
#      includes), enabled, tested and reloaded.
# It touches no other site. Re-running it is safe.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)

if [ "$(id -u)" -ne 0 ]; then
  echo "Run with sudo." >&2
  exit 1
fi
if [ ! -d /home/forge/navree.com/current ]; then
  echo "/home/forge/navree.com/current is missing: run deploy.sh first." >&2
  exit 1
fi

# The Cloudflare credentials file the other certificates renew with, unless
# CF_CREDENTIALS points at another one. Its token must be allowed to edit DNS
# for navree.com (Cloudflare → My Profile → API Tokens → Edit the token →
# Zone Resources: include navree.com).
creds="${CF_CREDENTIALS:-$(grep -h '^dns_cloudflare_credentials' /etc/letsencrypt/renewal/*.conf | head -1 | cut -d= -f2 | xargs)}"
echo "Using Cloudflare credentials: $creds"

echo "1/2 Certificate for navree.com and www.navree.com"
certbot certonly \
  --dns-cloudflare --dns-cloudflare-credentials "$creds" \
  -d navree.com -d www.navree.com \
  --non-interactive --keep-until-expiring

echo "2/2 nginx site"
install -d -m 755 /etc/nginx/snippets
install -m 644 "$here/security-headers.conf" /etc/nginx/snippets/navree.com-security.conf
install -m 644 "$here/nginx.conf" /etc/nginx/sites-available/navree.com
ln -sfn /etc/nginx/sites-available/navree.com /etc/nginx/sites-enabled/navree.com
nginx -t
systemctl reload nginx

echo "Done: nginx serves navree.com from /home/forge/navree.com/current."
