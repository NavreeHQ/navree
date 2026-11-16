// document_script_harness.js <script.js> [change]: runs the document script in a stand-in page, lets the page change
// what it likes after the script ran (the [change] named), then tries to open sockets and start workers, some of them
// with code of the page's own that runs while one is being made.
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const change = process.argv[3] || "none";

// The harness's own references, taken before anything runs: a change the page makes never reaches them.
const Str = String;
const RealURL = URL;
const stringify = JSON.stringify;
const write = process.stdout.write.bind(process.stdout);
const push = (list, value) => { list[list.length] = value; };
const each = (list, f) => { const out = []; for (let i = 0; i < list.length; i++) out[out.length] = f(list[i]); return out; };
const keysOf = Object.keys;
const protoOf = Object.getPrototypeOf;
const isArray = Array.isArray;
const apply = Reflect.apply;
const hrefOf = Object.getOwnPropertyDescriptor(URL.prototype, "href").get;
const resolve = (u) => apply(hrefOf, new RealURL(Str(u), page.base), []);

// The page's base address: what the document's real base is, as the engine resolves against it.
const page = { base: "http://localhost:8080/" };

const sockets = [];
const workers = [];
const protocols = [];
const options = [];
const given = [];
// The engine's own constructors. Every argument is converted first, as the engine does it: the address is turned into
// a string, a list of protocols is read through its iterator and each one turned into a string, and each field of a
// worker's options is read and turned into a string. Only then is the address resolved against the document's real
// base, as it is at that moment.
const readList = (p) => { if (p === null || typeof p !== "object") return; for (const v of p) Str(v); };
const readFields = (o) => {
  if (o === null || typeof o !== "object") return;
  const names = ["credentials", "name", "type"];
  for (let i = 0; i < names.length; i++) { const v = o[names[i]]; if (v !== undefined) Str(v); }
};
class FakeWebSocket {
  constructor(u, p) { const address = Str(u); readList(p); push(given, u); push(sockets, resolve(address)); push(protocols, p); }
}
FakeWebSocket.CONNECTING = 0; FakeWebSocket.OPEN = 1; FakeWebSocket.CLOSING = 2; FakeWebSocket.CLOSED = 3;
class FakeWorker {
  constructor(u, o) { const address = Str(u); readFields(o); push(given, u); push(workers, resolve(address)); push(options, o); }
}
class FakeNode {
  get baseURI() { return page.base; }
}
class FakeDocument extends FakeNode {}
global.self = global;
global.WebSocket = FakeWebSocket;
global.Worker = FakeWorker;
global.Node = FakeNode;
global.document = new FakeDocument();
global.document.documentElement = { remove() {} };
global.location = { href: "http://localhost:8080/" };
global.origin = "http://localhost:8080";
global.window = { stop() {} };
global.MutationObserver = class { observe() {} };

// The object the web view gives a document of the app, to say where it is.
const posted = [];
global.navreeSeen = { postMessage(m) { push(posted, m); } };

let emptied = false;
global.document.documentElement.remove = () => { emptied = true; };
(0, eval)(source);

// What a page's script does after the document script ran. Each answers the app's address, or "yes", wherever the
// document script might ask it something, so a check that asks it lets another host through.
const own = "http://localhost:8080/";
const changes = {
  none() {},
  url() {
    global.URL = function () { return { href: own, protocol: "http:", hostname: "localhost", port: "8080", origin: "http://localhost:8080" }; };
  },
  string() {
    global.String = function (u) { return u; };
  },
  "string methods"() {
    String.prototype.replace = function () { return own; };
    String.prototype.slice = function () { return "x"; };
    String.prototype.indexOf = function () { return 0; };
    String.prototype.startsWith = function () { return true; };
    String.prototype.endsWith = function () { return true; };
    String.prototype.toLowerCase = function () { return "localhost"; };
  },
  "url getters"() {
    for (const [name, value] of [["href", own], ["protocol", "http:"], ["hostname", "localhost"], ["host", "localhost:8080"], ["port", "8080"], ["origin", "http://localhost:8080"]]) {
      Object.defineProperty(RealURL.prototype, name, { get() { return value; }, configurable: true });
    }
  },
  base() {
    // A base element that sends a relative address elsewhere, and a base address that says it is the app's.
    page.base = "ws://other.example:8080/";
    Object.defineProperty(FakeNode.prototype, "baseURI", { get() { return own; }, configurable: true });
  },
  "call and apply"() {
    Function.prototype.call = function () { return own; };
    Function.prototype.apply = function () { return own; };
    Function.prototype.bind = function () { return function () { return own; }; };
  },
  reflect() {
    Reflect.apply = function () { return own; };
    Reflect.construct = function () { return { href: own, protocol: "http:", hostname: "localhost", port: "8080" }; };
    Reflect.get = function () { return own; };
  },
  arrays() {
    Array.from = function () { return ["x"]; };
    Array.prototype.indexOf = function () { return 0; };
    Array.prototype.includes = function () { return true; };
    Array.prototype.some = function () { return true; };
    Array.prototype.forEach = function () {};
    Array.prototype.map = function () { return []; };
  },
  "domexception"() {
    global.DOMException = function () { return undefined; };
  },
  regexp() {
    RegExp.prototype[Symbol.replace] = function () { return own; };
    RegExp.prototype.test = function () { return true; };
    RegExp.prototype.exec = function () { return [own]; };
  },
  "array iterator"() {
    // What every list is read with, the engine's reading of the list it is handed too: here it also sends a relative
    // address elsewhere from then on.
    const real = Array.prototype[Symbol.iterator];
    Array.prototype[Symbol.iterator] = function () { page.base = "ws://other.example:8080/"; return apply(real, this, []); };
  },
};
changes[change]();

const results = {};
function attempt(name, fn) {
  try { fn(); results[name] = "made"; } catch (e) { results[name] = "refused " + (e && e.name); }
}

// a plain address of the app, and other hosts (one on the app's own port)
attempt("own socket", () => new WebSocket("ws://localhost:8080/own"));
attempt("other socket", () => new WebSocket("ws://other.example/x"));
attempt("other socket on the app's port", () => new WebSocket("ws://other.example:8080/x"));
attempt("relative socket", () => new WebSocket("x"));
// a host under a pattern the app names (`*.cdn.example`), the name itself, and one that only ends the same way
attempt("host under a pattern", () => new WebSocket("wss://a.cdn.example/x"));
attempt("the pattern's own name", () => new WebSocket("wss://cdn.example/x"));
attempt("a name that ends the same", () => new WebSocket("wss://evilcdn.example/x"));
// an object whose conversion answers the app's address first and another host second
let n = 0;
attempt("toString socket", () => new WebSocket({ toString() { return n++ ? "ws://other.example/x" : "ws://localhost:8080/own"; } }));
let p = 0;
attempt("toPrimitive socket", () => new WebSocket({ [Symbol.toPrimitive]() { return p++ ? "ws://other.example/x" : "ws://localhost:8080/own"; } }));
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
const last = options.length ? options[options.length - 1] : null;

// What the page's own code can do while a socket or a worker is being made: put another base into the document. The
// base is put back after each, so the next starts where this one did.
function moving(name, fn) {
  const before = page.base;
  attempt(name, fn);
  page.base = before;
}
const elsewhere = "http://other.example:8080/";
moving("base moved while the protocols are read", () => new WebSocket("x1", { [Symbol.iterator]: function* () { page.base = elsewhere; yield "a"; } }));
moving("base moved while a worker's options are read", () => new Worker("moved.js", { get name() { page.base = elsewhere; return "w"; } }));
moving("base moved while a worker's option is turned into a string", () => new Worker("late.js", { name: { toString() { page.base = elsewhere; return "w"; } } }));
moving("a blob base set while a worker's options are read", () => new Worker("#", { get type() { page.base = "blob:http://localhost:8080/abc"; return "classic"; } }));
// The list the real constructor is handed is read by the engine through the arrays' own iterator, which the page
// replaced: the first reading is the document script's copy, and from the second the base is moved.
moving("base moved while the engine reads the list it is handed", () => {
  const real = Array.prototype[Symbol.iterator];
  let reads = 0;
  Array.prototype[Symbol.iterator] = function () { if (reads++) page.base = elsewhere; return apply(real, this, []); };
  try { return new WebSocket("x2", ["a"]); } finally { Array.prototype[Symbol.iterator] = real; }
});

const kindsOf = (o) => each(keysOf(o), (k) => typeof o[k]);
const plain = (o) => o !== undefined && o !== null && typeof o === "object" && !isArray(o) ? { plain: protoOf(o) === Object.prototype, keys: keysOf(o), kinds: kindsOf(o) } : o;
const handed = each(given, (g) => (typeof g === "string" ? g : "not a string"));
write(stringify({ change, results, sockets, workers, emptied, posted, given: handed, protocols: each(protocols, (q) => (isArray(q) ? q : plain(q))), options: each(options, plain), optionName: last && last.name }) + "\n");
