// site_wipe_harness.js <script.js> [case]: runs the script the client puts at the start of its own document when a
// session has ended, in a stand-in for a document of one origin that holds service worker registrations, caches,
// databases and local storage, and prints what the script did and what it said through its listener.
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const which = process.argv[3] || "a worker and two caches";

const seen = { messages: [], stopped: false, emptied: false, observed: false, unregistered: 0, deleted: [], dropped: [], emptiedStorage: 0 };

// What the origin holds, by case.
let registrations = [];
let names = [];
let databases = [];
let stored = {};
let rejects = false;
let keeps = false;
let blocked = false;
let again = false;
let putBack = false;
let unreachable = false;
let listener = true;
let frame = false;
let apis = true;
switch (which) {
  case "a worker and two caches":
    registrations.push("/sw.js");
    names = ["pages", "orders"];
    break;
  case "everything a page stores":
    registrations.push("/sw.js");
    names = ["pages"];
    databases = ["orders", "drafts"];
    stored = { city: "Ludhiana", lane: "open" };
    break;
  case "nothing stored":
    break;
  case "no worker and no cache interface":
    apis = false;
    break;
  case "the registrations cannot be asked for":
    registrations.push("/sw.js");
    names = ["pages"];
    rejects = true;
    break;
  case "a cache that stays":
    names = ["pages"];
    keeps = true;
    break;
  case "a database that stays":
    databases = ["orders"];
    blocked = true;
    break;
  case "a worker registered again":
    registrations.push("/sw.js");
    again = true;
    break;
  case "a cache put back while the script waits":
    registrations.push("/sw.js");
    names = ["pages"];
    putBack = true;
    break;
  case "local storage that cannot be reached":
    names = ["pages"];
    unreachable = true;
    break;
  case "no listener":
    registrations.push("/sw.js");
    listener = false;
    break;
  case "a frame":
    registrations.push("/sw.js");
    names = ["pages"];
    frame = true;
    break;
  default:
    throw new Error("no such case: " + which);
}

global.self = global;
global.top = frame ? {} : global;
global.window = { stop() { seen.stopped = true; } };
global.MutationObserver = class { constructor(f) { this.f = f; } observe() { seen.observed = true; } };
global.document = { documentElement: { remove() { seen.emptied = true; } } };
if (listener) global.navreeWipe = { postMessage(message) { seen.messages.push(message); } };
const registration = (scope) => ({
  unregister() {
    seen.unregistered += 1;
    // A page that is still open registers the same scope again at once.
    registrations = again ? ["/sw.js"] : registrations.filter((other) => other !== scope);
    return Promise.resolve(true);
  },
});
const worker = {
  getRegistrations() {
    if (rejects) return Promise.reject(new Error("InvalidStateError: The document is in an invalid state."));
    return Promise.resolve(registrations.map(registration));
  },
};
// Node has a `navigator` of its own: the stand-in takes its place.
Object.defineProperty(globalThis, "navigator", { value: apis ? { serviceWorker: worker } : {}, configurable: true, writable: true });
if (apis) {
  global.caches = {
    keys() { return Promise.resolve(names.slice()); },
    delete(name) {
      seen.deleted.push(name);
      if (!keeps) names = names.filter((other) => other !== name);
      // A worker of the session that is still running a request stores its answer a little later.
      if (putBack) setTimeout(() => { names = names.concat(["late"]); }, 100);
      return Promise.resolve(!keeps);
    },
  };
  global.indexedDB = {
    databases() { return Promise.resolve(databases.map((name) => ({ name, version: 1 }))); },
    deleteDatabase(name) {
      seen.dropped.push(name);
      const request = {};
      setTimeout(() => {
        if (blocked) { if (request.onblocked) request.onblocked(); return; }
        databases = databases.filter((other) => other !== name);
        if (request.onsuccess) request.onsuccess();
      }, 5);
      return request;
    },
  };
  const storage = {
    get length() { return Object.keys(stored).length; },
    clear() { seen.emptiedStorage += 1; stored = {}; },
  };
  Object.defineProperty(globalThis, "localStorage", {
    get() { if (unreachable) throw new Error("SecurityError: storage is not available"); return storage; },
    configurable: true,
  });
}

(0, eval)(source);

// The script's work is promises and one wait of its own: what it said is read once they have settled.
setTimeout(() => {
  seen.left = names;
  seen.databases = databases;
  seen.stored = Object.keys(stored).length;
  process.stdout.write(JSON.stringify(seen));
}, 600);
