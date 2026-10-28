// Conformance of navree-mcp's server with the official MCP TypeScript SDK
// client (@modelcontextprotocol/sdk, pinned in package.json): the handshake,
// listing and calling tools (results, tool errors, protocol errors,
// progress), listing and reading resources and templates, and listing and
// getting prompts, over stdio and over Streamable HTTP.
//
// Started by `cargo test -p navree-mcp --test sdk_conformance -- --ignored`,
// which boots the server and passes:
//   MCP_HTTP_URL     the Streamable HTTP endpoint (http://127.0.0.1:<port>/mcp)
//   MCP_TOKEN        a bearer token for it
//   MCP_STDIO_PORT   the TCP port where it serves the stdio transport
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const sdkVersion = JSON.parse(
  readFileSync(path.join(here, 'node_modules/@modelcontextprotocol/sdk/package.json'), 'utf8'),
).version;

function required(name) {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} is not set: run this through cargo test (see the header)`);
  }
  return value;
}

/** Runs every check through `transport`. */
async function exercise(label, transport) {
  const client = new Client({ name: 'navree-conformance', version: '1.0.0' });
  await client.connect(transport);

  // The handshake.
  const server = client.getServerVersion();
  assert.equal(server.name, 'navree-sdk');
  assert.equal(server.title, 'Navree SDK conformance');
  const capabilities = client.getServerCapabilities();
  assert.equal(capabilities.tools?.listChanged, true);
  assert.equal(capabilities.resources?.subscribe, true);
  assert.equal(capabilities.prompts?.listChanged, true);
  assert.equal(client.getInstructions(), 'Greets people and keeps notes.');
  await client.ping();

  // Tools.
  const { tools } = await client.listTools();
  const names = tools.map((tool) => tool.name).sort();
  assert.deepEqual(names, ['count', 'greet']);
  const greet = tools.find((tool) => tool.name === 'greet');
  assert.equal(greet.description, 'Greets someone by name.');
  assert.equal(greet.inputSchema.type, 'object');
  assert.equal(greet.inputSchema.properties.name.description, 'Who to greet.');
  assert.equal(greet.annotations.readOnlyHint, true);

  const greeted = await client.callTool({ name: 'greet', arguments: { name: 'Ada' } });
  assert.equal(greeted.isError, false);
  assert.equal(greeted.structuredContent.greeting, 'Hello, Ada!');
  assert.equal(JSON.parse(greeted.content[0].text).greeting, 'Hello, Ada!');

  // A tool error is a result marked isError, with Navree's error body.
  const refused = await client.callTool({ name: 'greet', arguments: { name: '' } });
  assert.equal(refused.isError, true);
  assert.equal(refused.structuredContent.statusCode, 400);

  // An unknown tool is a protocol error.
  await assert.rejects(
    client.callTool({ name: 'no_such_tool', arguments: {} }),
    (error) => error.code === -32602 && /Unknown tool: no_such_tool/.test(error.message),
  );

  // Progress notifications reach the caller before the result.
  const progress = [];
  const counted = await client.callTool({ name: 'count', arguments: {} }, undefined, {
    onprogress: (update) => progress.push(update),
  });
  assert.equal(counted.isError, false);
  assert.equal(counted.structuredContent.result, 2);
  // The SDK handles notifications a microtask after reading them but the
  // answer at once, so a progress notification that arrives in the same
  // chunk as the answer can be dropped on the client: expect the first.
  assert.ok(progress.length >= 1, 'progress arrives');
  assert.deepEqual([progress[0].progress, progress[0].total], [1, 2]);
  assert.equal(progress[0].message, 'one');

  // Resources and templates.
  const { resources } = await client.listResources();
  assert.deepEqual(
    resources.map((resource) => resource.uri),
    ['notes://welcome'],
  );
  const { resourceTemplates } = await client.listResourceTemplates();
  assert.deepEqual(
    resourceTemplates.map((template) => template.uriTemplate),
    ['notes://items/{id}'],
  );
  const welcome = await client.readResource({ uri: 'notes://welcome' });
  assert.equal(welcome.contents[0].text, 'Welcome to the notes.');
  assert.equal(welcome.contents[0].mimeType, 'text/plain');
  const item = await client.readResource({ uri: 'notes://items/42' });
  assert.equal(JSON.parse(item.contents[0].text).id, 42);
  assert.equal(item.contents[0].mimeType, 'application/json');
  await assert.rejects(client.readResource({ uri: 'notes://nowhere' }), (error) => error.code === -32002);

  // Prompts.
  const { prompts } = await client.listPrompts();
  const review = prompts.find((prompt) => prompt.name === 'review');
  assert.deepEqual(review.arguments, [{ name: 'topic', description: 'What to review.', required: true }]);
  const rendered = await client.getPrompt({ name: 'review', arguments: { topic: 'MCP' } });
  assert.equal(rendered.messages[0].role, 'user');
  assert.equal(rendered.messages[0].content.text, 'Review MCP in three sentences.');
  assert.equal(rendered.messages[1].role, 'assistant');

  await client.close();
  console.log(`ok ${label}: initialize, tools, resources, prompts`);
}

console.log(`@modelcontextprotocol/sdk ${sdkVersion}`);

await exercise(
  'stdio',
  new StdioClientTransport({
    command: process.execPath,
    args: [path.join(here, 'stdio-bridge.mjs'), required('MCP_STDIO_PORT')],
    stderr: 'inherit',
  }),
);

await exercise(
  'streamable-http',
  new StreamableHTTPClientTransport(new URL(required('MCP_HTTP_URL')), {
    requestInit: { headers: { Authorization: `Bearer ${required('MCP_TOKEN')}` } },
  }),
);

console.log('conformance: all checks passed');
