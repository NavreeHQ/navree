// Relays this process's stdin and stdout, byte for byte, to a TCP port where
// the conformance test serves navree-mcp's stdio transport
// (`serve_stdio`). The SDK's StdioClientTransport starts this script as its
// server process, so it speaks newline-delimited JSON-RPC to Navree exactly
// as it would to `mcp:serve`.
import net from 'node:net';

const port = Number(process.argv[2]);
if (!Number.isInteger(port)) {
  process.stderr.write('usage: node stdio-bridge.mjs <port>\n');
  process.exit(2);
}

const socket = net.connect({ host: '127.0.0.1', port });
process.stdin.pipe(socket);
socket.pipe(process.stdout);
socket.on('error', (error) => {
  process.stderr.write(`stdio bridge: ${error.message}\n`);
  process.exit(1);
});
socket.on('close', () => process.exit(0));
