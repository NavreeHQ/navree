// site_wipe_harness.js <script.js> [case]: runs the script the client puts at the start of its own document when a
// session has ended, in a stand-in for a document of one origin that holds service worker registrations and caches,
// and prints what the script did and what it said through its listener.
const fs = require("fs");
const source = fs.readFileSync(process.argv[2], "utf8");
const which = process.argv[3] || "a worker and two caches";

const seen = { messages: [], stopped: false, emptied: false, observed: false, unregistered: 0, deleted: [] };

// What the origin holds, by case.
const registrations = [];
let names = [];
let rejects = false;
let keeps = false;
let listener = true;
let frame = false;
let apis = true;
switch (which) {
  case "a worker and two caches":
    registrations.push("/sw.js");
    names = ["pages", "orders"];
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
const worker = {
  getRegistrations() {
    if (rejects) return Promise.reject(new Error("InvalidStateError: The document is in an invalid state."));
    return Promise.resolve(registrations.map(() => ({ unregister() { seen.unregistered += 1; return Promise.resolve(true); } })));
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
      return Promise.resolve(!keeps);
    },
  };
}

(0, eval)(source);

// The script's work is promises: what it said is read once they have settled.
setTimeout(() => {
  seen.left = names;
  process.stdout.write(JSON.stringify(seen));
}, 50);
