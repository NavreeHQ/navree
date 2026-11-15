// document_script_harness.js <script.js>: runs the document script in a stand-in page and tries the page's tricks.
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");

const sockets = [];
const workers = [];
const protocols = [];
const options = [];
class FakeWebSocket {
  constructor(u, p) { sockets.push(String(u)); protocols.push(p); }
}
FakeWebSocket.CONNECTING = 0; FakeWebSocket.OPEN = 1; FakeWebSocket.CLOSING = 2; FakeWebSocket.CLOSED = 3;
class FakeWorker {
  constructor(u, o) { workers.push(String(u)); options.push(o); }
}
global.self = global;
global.WebSocket = FakeWebSocket;
global.Worker = FakeWorker;
global.document = { baseURI: "http://localhost:8080/", documentElement: { remove() {} } };
global.location = { href: "http://localhost:8080/" };
global.origin = "http://localhost:8080";
global.window = { stop() {} };
global.MutationObserver = class { observe() {} };

let emptied = false;
global.document.documentElement.remove = () => { emptied = true; };
(0, eval)(source);

const results = {};
function attempt(name, fn) {
  try { fn(); results[name] = "made"; } catch (e) { results[name] = "refused " + (e && e.name); }
}

// a plain address of the app, and another host
attempt("own socket", () => new WebSocket("ws://localhost:8080/own"));
attempt("other socket", () => new WebSocket("ws://other.example/x"));
// an object whose conversion answers the app's address first and another host second
let n = 0;
attempt("toString socket", () => new WebSocket({ toString() { return n++ ? "ws://other.example/x" : "ws://localhost:8080/own"; } }));
n = 0;
attempt("toString worker", () => new Worker({ toString() { return n++ ? "blob:http://localhost:8080/abc" : "/own.js"; } }));
attempt("blob worker", () => new Worker("blob:http://localhost:8080/abc"));
attempt("data worker", () => new Worker("data:text/javascript,1"));
attempt("own worker", () => new Worker("/own.js", { name: "w", type: "classic" }));
attempt("constructor", () => new (new WebSocket("ws://localhost:8080/own").constructor)("ws://other.example/x"));
// options: protocols and an object whose fields answer differently
let m = 0;
attempt("protocols", () => new WebSocket("ws://localhost:8080/p", { [Symbol.iterator]: function* () { yield "a"; yield "b"; } }));
attempt("options", () => new Worker("/o.js", { get name() { return "n" + m++; }, type: "module", credentials: "omit", extra: 1 }));

const own = (o) => o !== undefined && o !== null && typeof o === "object" && !Array.isArray(o) ? { plain: Object.getPrototypeOf(o) === Object.prototype, keys: Object.keys(o) } : o;
console.log(JSON.stringify({ results, sockets, workers, emptied, protocols: protocols.map((p) => (Array.isArray(p) ? p : own(p))), options: options.map(own), optionName: options.length ? options[options.length - 1] && options[options.length - 1].name : null }));
