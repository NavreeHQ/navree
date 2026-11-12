// Serves the example envelopes over HTTP so the demo app has something to
// talk to when no Navree app is running: `node fixtures/serve.mjs [port]`.
// From the Android emulator the address is http://10.0.2.2:<port>.
//
// It answers what the examples can answer and nothing else: pages by address,
// a search that filters the rows of a list, the next page of the orders, and
// posts that answer field errors or a toast and a place to go.
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const port = Number(process.argv[2] ?? 8787);
const NATIVE = 'application/vnd.navree.native+json';

const read = (name) => JSON.parse(readFileSync(join(here, name), 'utf8'));
const envelope = (name) => read(`envelopes/${name}.json`);

const pages = [
  [/^\/$/, 'home'],
  [/^\/orders$/, 'orders'],
  [/^\/orders\/new$/, 'order-new'],
  [/^\/orders\/\d+\/edit$/, 'order-edit'],
  [/^\/orders\/\d+$/, 'order'],
  [/^\/products$/, 'products'],
  [/^\/customers$/, 'customers'],
  [/^\/settings$/, 'settings'],
  [/^\/sign-in$/, 'sign-in'],
  [/^\/reports$/, 'web'],
];

// The rows of the first list in a tree, kept when their words hold the query.
function filtered(node, query) {
  if (Array.isArray(node)) return node.map((child) => filtered(child, query));
  if (node === null || typeof node !== 'object') return node;
  const copy = Object.fromEntries(Object.entries(node).map(([key, value]) => [key, filtered(value, query)]));
  if (copy.t === 'list') {
    copy.children = copy.children.filter((row) => JSON.stringify(row).toLowerCase().includes(query));
    delete copy.more;
  }
  return copy;
}

function body(request) {
  return new Promise((resolve) => {
    let text = '';
    request.on('data', (chunk) => (text += chunk));
    request.on('end', () => resolve(new URLSearchParams(text)));
  });
}

function send(response, status, value) {
  response.writeHead(status, { 'Content-Type': `${NATIVE}; charset=utf-8`, 'Cache-Control': 'no-store' });
  response.end(JSON.stringify(value));
}

createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  const path = url.pathname;
  const native = request.headers['x-navree-native'] === '1';
  console.log(request.method, request.url, native ? '' : '(web)');

  if (path === '/_navree/native/shell') return send(response, 200, read('shell.json'));

  if (!native) {
    // The web view asks for the page itself: a plain page stands in for the app's HTML.
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    return response.end(
      `<!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><title>Reports</title>` +
        `<body style="font: 16px system-ui; margin: 16px"><h1 style="font-size: 20px">Sales this week</h1>` +
        `<p>This page has a chart, so it is shown as a web page inside the app.</p>` +
        `<p><a href="/orders/1042">Order #1042</a> · <a href="https://navree.com/docs">Documentation</a></p></body>`,
    );
  }

  if (request.method === 'POST') {
    const fields = await body(request);
    if (/^\/orders(\/\d+)?$/.test(path)) {
      const phone = (fields.get('phone') ?? '').replace(/\D/g, '');
      const name = (fields.get('name') ?? '').trim();
      if (phone.length !== 10 || name === '') {
        const errors = envelope('ops-errors');
        errors.ops[0].form = path === '/orders' ? 'new-order-form' : 'order-form';
        errors.ops[0].fields = {
          ...(name === '' ? { name: ["Enter the customer's name"] } : {}),
          ...(phone.length !== 10 ? { phone: ['Enter a 10-digit mobile number'] } : {}),
        };
        errors.ops = errors.ops.filter((op) => op.op !== 'focus');
        return send(response, 422, errors);
      }
      return send(response, 200, envelope('ops-saved'));
    }
    if (/^\/orders\/\d+\/ship$/.test(path)) return send(response, 200, envelope('ops-shipped'));
    if (/^\/orders\/\d+\/cancel$/.test(path)) {
      return send(response, 200, { navree: 1, ops: [{ op: 'toast', tone: 'success', text: 'Order cancelled' }, { op: 'navigate', href: '/orders' }] });
    }
    return send(response, 200, { navree: 1, ops: [{ op: 'toast', tone: 'success', text: 'Done' }] });
  }

  if (path === '/orders' && url.searchParams.has('after')) return send(response, 200, envelope('orders-more'));

  const page = pages.find(([pattern]) => pattern.test(path));
  if (!page) return send(response, 404, { navree: 1, screen: { title: 'Not found', body: { t: 'empty', title: 'This page does not exist' } } });
  let answer = envelope(page[1]);
  const query = (url.searchParams.get('q') ?? '').trim().toLowerCase();
  if (query !== '' && answer.screen) {
    answer = { ...answer, screen: { ...filtered(answer.screen, query), search: { ...answer.screen.search, value: url.searchParams.get('q') } } };
  }
  return send(response, 200, answer);
}).listen(port, () => console.log(`Example envelopes on http://localhost:${port}`));
