#!/bin/sh
# Real terminal output of Navree's command line tools, for the UI evidence
# gallery: Harbor (what it can do without a server) and Navigator (the MCP
# server, over stdio). Runs inside the image `navree-cli:local`
# (e2e/docker/Dockerfile.cli), which holds the `navree` binary and the
# repository's source; writes one text file per command to /captures. The
# first line of each is the command, the last the exit status.
#
#   docker run --rm -v "$PWD/captures:/captures" navree-cli:local sh /src/e2e/tools/cli-captures.sh
#
# Harbor's `deploy` needs a server reachable over SSH, and Navigator's route
# and module tools need the app built, so those are shown the way they answer
# without them.

set -u
mkdir -p /captures
export NO_COLOR=1 TERM=dumb

cap() {
  name="$1"; shift
  {
    echo "\$ $*"
    "$@" 2>&1
    echo "[exit $?]"
  } > "/captures/$name.txt"
}

# --- Harbor: in a scratch copy of an app ---------------------------------
rm -rf /tmp/app && cp -r /src/apps/crm-api /tmp/app && cd /tmp/app || exit 1
rm -f harbor.toml harbor.key
cap harbor-01-help navree --help
cap harbor-02-init navree harbor:init
cap harbor-03-secrets-set navree harbor:secrets set APP_KEY --generate
cap harbor-04-secrets-list navree harbor:secrets list
cap harbor-05-ci navree harbor:ci github
{
  cat <<'TOML'
app = "crm-api"
domain = "crm.example.com"

[servers.app-1]
host = "127.0.0.1"
roles = ["web", "worker", "scheduler", "postgres", "redis"]

TOML
  # The encrypted secrets that harbor:secrets wrote.
  sed -n '/^\[secrets\]/,$p' harbor.toml
} > harbor.toml.new
mv harbor.toml.new harbor.toml
cap harbor-06-releases-no-server navree deploy:releases

# --- Navigator: a session over stdio -------------------------------------
cd /src/apps/evidence-app || exit 1
python3 - <<'PY' > /captures/navigator-01-session.txt 2>&1
import json, subprocess, time

cmd = ['navree', 'mcp', '--path', '/src/apps/evidence-app', '--no-build', '--docs-offline']
print('$ navree mcp --path apps/evidence-app --no-build --docs-offline')
print('  (an agent\'s session on stdio: each call, and what Navigator answered)\n')
p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def ask(i, method, params=None):
    p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': i, 'method': method, 'params': params or {}}) + '\n')
    p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line:
            return None
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get('id') == i:
            return message


def text_of(message):
    result = (message or {}).get('result', {})
    if 'content' in result:
        return '\n'.join(c.get('text', '') for c in result['content'])
    return json.dumps(message, indent=2)


def clip(text, lines=42):
    rows = text.split('\n')
    return '\n'.join(rows[:lines] + (['  ... (%d more lines)' % (len(rows) - lines)] if len(rows) > lines else []))


init = ask(1, 'initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'capture', 'version': '1'}})
info = init['result']['serverInfo']
print('> initialize')
print('  %s %s, protocol %s\n' % (info.get('title'), info.get('version'), init['result']['protocolVersion']))
p.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
p.stdin.flush()

tools = ask(2, 'tools/list')['result']['tools']
print('> tools/list: %d tools' % len(tools))
for t in tools:
    first = t.get('description', '').split('. ')[0].replace('\n', ' ')
    print('  %-26s %s' % (t['name'], first[:90]))
print()

for n, (name, args) in enumerate([('app_info', {}), ('list_routes', {}), ('read_config', {}), ('last_error', {})], start=3):
    print('> tools/call %s' % name)
    reply = ask(n, 'tools/call', {'name': name, 'arguments': args})
    print(clip(text_of(reply)))
    print()
p.stdin.close()
try:
    p.wait(timeout=20)
except Exception:
    p.kill()
print('[exit %s]' % p.returncode)
PY
