// Renders captured terminal output (text files) as images, so the command
// line tools can sit in the gallery beside the tools that have pages. The
// text is exactly what the command printed; only the frame is added.
//
//   IN_DIR=/in OUT_DIR=/out node terminal.mjs
//
// Every <name>.txt in IN_DIR becomes <name>.png in OUT_DIR. The first line
// of a capture that starts with "$ " is the command, shown as the title.

import { chromium } from 'playwright';
import { mkdir, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';

const IN_DIR = process.env.IN_DIR || './captures';
const OUT_DIR = process.env.OUT_DIR || './shots/cli';
await mkdir(OUT_DIR, { recursive: true });

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const strip = (s) => s.replace(/\u001b\[[0-9;]*[A-Za-z]/g, '');

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1100, height: 600 } });
for (const file of (await readdir(IN_DIR)).filter((f) => f.endsWith('.txt')).sort()) {
  const text = strip(await readFile(path.join(IN_DIR, file), 'utf8')).replace(/\s+$/, '');
  const lines = text.split('\n');
  const title = lines[0].startsWith('$ ') ? lines[0].slice(2) : file;
  const body = lines[0].startsWith('$ ') ? lines.slice(1).join('\n') : text;
  await page.setContent(`<!doctype html><meta charset="utf-8"><style>
    body { margin: 0; background: #0b0d0c; font: 13px/1.5 ui-monospace, Consolas, monospace; color: #dfe6e2; }
    .bar { padding: 8px 14px; background: #161a18; border-bottom: 1px solid #2a312e; color: #8f9c96; }
    .bar b { color: #39d98a; font-weight: 600; }
    pre { margin: 0; padding: 14px; white-space: pre-wrap; word-break: break-word; }
  </style><div class="bar"><b>$</b> ${esc(title)}</div><pre>${esc(body)}</pre>`);
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  await page.setViewportSize({ width: 1100, height: Math.min(Math.max(height, 120), 12000) });
  await page.screenshot({ path: path.join(OUT_DIR, file.replace(/\.txt$/, '.png')) });
  console.log(`ok ${file}`);
}
await browser.close();
