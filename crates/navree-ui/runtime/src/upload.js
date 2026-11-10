// The upload module. A native file input in a form already posts, so this adds what it lacks:
// a drop area, a list of the files with progress, a way to stop one and to send it again, and
// large files sent in chunks. It is delegated from `document` and reads the page through dom.js,
// so a morph needs no wiring and a name in stored markup can't shadow a method. The limits in
// the markup are hints; the server holds every file to them. Text from the server goes in with
// textContent, never as markup.
import { T, all, attr, byId, closest, cookie, every, has, holds, on, set, unset } from "./dom.js";

const ROOT = "[data-ui-upload]";
const ROW = "[data-slot=upload-item]";

/** @param {Element} root @param {string} name */
const part = (root, name) => all(root, "[data-slot=" + name + "]")[0];
/** @param {Element} root @returns {HTMLInputElement} */
const field = (root) => /** @type {any} */ (all(root, "input[type=file]")[0]);
/** @param {Element} root @param {string} name */
const num = (root, name) => Number(attr(root, name)) || 0;
/** Where a root sends its files: its own form's action, or, in a form of the page's, the route
 * that stores a file and answers with its key. @param {Element} root */
const target = (root) => attr(root, "action") || attr(root, "data-send") || "";
/** @param {Element} root */
const auto = (root) => !!target(root) && !has(root, "data-manual");
/** The hidden field that carries the key of the file a form's upload sent. @param {Element} root */
const keyField = (root) => /** @type {HTMLInputElement | undefined} */ (/** @type {any} */ (part(root, "upload-key")));
/** Whether a file of `root` is on its way or waiting to go. @param {Element} root */
const busy = (root) => all(root, ROW + "[data-state=sending]," + ROW + "[data-state=queued]")[0];
/**
 * In a form of the page's, the file field is required only while it has neither a key nor a file
 * on its way: the form then waits for the key, and says so, rather than the browser asking for a
 * file the field has already sent. @param {Element} root
 */
function needs(root) {
  const key = keyField(root);
  if (key) field(root).required = !key.value && !busy(root) && has(root, "data-required");
}
/** @param {number} n */
const size = (n) => {
  const unit = (v, u) => v.toFixed(1).replace(/\.0$/, "") + " " + u;
  return n < 1024 ? n + " B" : n < 10240 ? unit(n / 1024, "KB") : n < 1048576 ? Math.round(n / 1024) + " KB" : unit(n / 1048576, "MB");
};

/** A word of the page, with its `{name}`s filled. @param {Element} root @param {string} key @param {Record<string, any>} [o] */
function say(root, key, o = {}) {
  let words = {};
  try { words = JSON.parse(attr(root, "data-l") || "{}"); } catch {}
  return String(words[key] ?? "").replace(/\{(\w+)\}/g, (_, k) => o[k] ?? "");
}
/** Says `text` to assistive technology. @param {Element} root @param {string} text */
const tell = (root, text) => { part(root, "upload-status").textContent = text; };

/** @typedef {{root: Element, file: File, li: Element, state: string, gone?: boolean, x?: XMLHttpRequest, id?: string}} Job */
/** Rows by their elements, the end of each root's line of sends, and how many are in it. */
const jobs = /** @type {WeakMap<Element, Job>} */ (new WeakMap());
const lines = /** @type {WeakMap<Element, Promise<void>>} */ (new WeakMap());
const waiting = /** @type {WeakMap<Element, number>} */ (new WeakMap());

/** Writes what a row says about its file, and whether its field still needs one. @param {Job} job @param {string} state @param {string} text */
function show(job, state, text) {
  job.state = state;
  set(job.li, "data-state", state);
  part(job.li, "attachment-description").textContent = text;
  needs(job.root);
}

/** Adds a row for `file`. @param {Element} root @param {File} file @param {string} state */
function add(root, file, state) {
  const template = /** @type {HTMLTemplateElement} */ (all(root, "template")[0]);
  const li = /** @type {Element} */ (template.content.firstElementChild?.cloneNode(true));
  /** @type {Job} */
  const job = { root, file, li, state };
  jobs.set(li, job);
  part(li, "attachment-title").textContent = file.name;
  for (const hidden of all(li, "[data-ui-sr]")) hidden.textContent = " " + file.name;
  part(root, "upload-list").append(li);
  show(job, state, size(file.size));
  if (has(root, "data-previews")) preview(job);
  return job;
}

/** A picture is shown small, read as a `data:` URL (the page's policy allows no `blob:`): not above 2 MB, and never an SVG. @param {Job} job */
function preview(job) {
  const img = part(job.li, "upload-preview");
  if (!img || !/^image\/(png|jpeg|gif|webp|avif)$/.test(job.file.type) || job.file.size > 2097152) return;
  const read = new FileReader();
  // A data URL is text: the reader read it as one.
  read.onload = () => { set(img, "src", /** @type {string} */ (read.result)); unset(img, "hidden"); };
  read.readAsDataURL(job.file);
}

/** What a file breaks, in words, or nothing. @param {Element} root @param {File} file */
function check(root, file) {
  const most = num(root, "data-max-files"), max = num(root, "data-max-bytes");
  if (!file.size) return say(root, "empty", { name: file.name });
  if (max && file.size > max) return say(root, "too_big", { name: file.name, max: size(max) });
  if (most && all(root, ROW + ":not([data-state=failed])").length >= most) return say(root, "too_many", { count: most });
  return "";
}

/** @param {Job} job @param {string} text */
function fail(job, text) {
  show(job, "failed", text);
  tell(job.root, text);
}

/**
 * One request. The answer is its status and its JSON, if it has any.
 * @param {Partial<Job>} job @param {string} method @param {string} url @param {any} body @param {string} type @param {(n: number) => void} [up]
 * @returns {Promise<{status: number, json: any}>}
 */
function req(job, method, url, body, type, up) {
  return new Promise((ok, no) => {
    const x = new XMLHttpRequest();
    job.x = x;
    x.open(method, url);
    x.setRequestHeader("accept", "application/json");
    const token = cookie("XSRF-TOKEN");
    if (token) x.setRequestHeader("x-xsrf-token", token);
    if (type) x.setRequestHeader("content-type", type);
    if (up) x.upload.onprogress = (e) => up(e.loaded);
    x.onload = () => {
      let json = {};
      try { json = JSON.parse(x.responseText); } catch {}
      ok({ status: x.status, json });
    };
    x.onerror = x.onabort = () => no(0);
    x.send(body);
  });
}

/** @param {Job} job @param {number} done */
function progress(job, done) {
  const pct = Math.min(100, Math.floor((done * 100) / job.file.size));
  set(all(job.li, "progress")[0], "value", /** @type {any} */ (pct));
  show(job, "sending", pct + "%");
}

/** The whole file in one post. @param {Job} job */
function whole(job) {
  const data = new FormData();
  const token = /** @type {HTMLInputElement | undefined} */ (/** @type {any} */ (all(job.root, "input[name=_csrf]")[0]));
  if (token) data.append("_csrf", token.value);
  data.append(field(job.root).name || "file", job.file);
  return req(job, "POST", target(job.root), data, "", (n) => progress(job, n));
}

/** The file in parts, each tried three times, then put together by the server. @param {Job} job */
async function chunked(job) {
  const base = attr(job.root, "data-chunks") || "";
  let r = await req(job, "POST", base, JSON.stringify({ name: job.file.name, size: job.file.size }), "application/json");
  const step = Number(r.json.chunk_bytes);
  if (r.status !== 200 || !(step > 0) || !r.json.id) return r;
  job.id = r.json.id;
  let done = 0;
  for (let at = 0, i = 0; at < job.file.size; at += step, i++) {
    const piece = job.file.slice(at, at + step);
    for (let tries = 0; ; tries++) {
      try {
        r = await req(job, "PUT", base + "/" + job.id + "/" + i, piece, "application/octet-stream", (n) => progress(job, done + n));
        if (r.status < 500) break;
      } catch (e) {
        if (job.gone) throw e;
      }
      if (tries > 1) throw 0;
      await new Promise((go) => setTimeout(go, 500 * 2 ** tries));
    }
    if (r.status >= 300) return r;
    done += piece.size;
  }
  return req(job, "POST", base + "/" + job.id + "/finish", "", "");
}

/** Sends one file, then lets the next go. It never throws. @param {Job} job */
async function run(job) {
  const root = job.root, file = job.file;
  if (!job.gone) {
    const words = say(root, "sending", { name: file.name });
    set(all(job.li, "progress")[0], "aria-label", words);
    show(job, "sending", "0%");
    tell(root, words);
    try {
      const r = await (attr(root, "data-chunks") && file.size >= num(root, "data-chunk-from") ? chunked(job) : whole(job));
      // In a form of the page's, the form posts the stored file's key in its place. Only such
      // a root has a key field, and one that sends at all sends to `data-send`.
      const key = keyField(root);
      const sent = r.json?.stored?.[0]?.key;
      if (r.status < 300 && key) {
        if (typeof sent === "string" && sent) key.value = sent;
        else r.status = 500;
      }
      if (r.status < 300) {
        const done = say(root, "done");
        show(job, "done", size(file.size) + " · " + done);
        tell(root, file.name + " · " + done);
      } else fail(job, r.json?.refused?.[0]?.message || r.json?.message || say(root, "failed", { name: file.name }));
    } catch {
      if (!job.gone) fail(job, say(root, "offline", { name: file.name }));
    }
  }
  const left = (waiting.get(root) || 1) - 1;
  waiting.set(root, left);
  // When the last file is over, the region that lists the files draws them again.
  const region = attr(root, "data-refresh");
  const target = !left && region ? byId(region) : null;
  if (target) T.dispatchEvent.call(target, new CustomEvent("ui:refresh", { bubbles: true }));
}

/** Puts `job` at the end of its root's line. @param {Job} job */
function enqueue(job) {
  const root = job.root;
  show(job, "queued", size(job.file.size));
  waiting.set(root, (waiting.get(root) || 0) + 1);
  lines.set(root, (lines.get(root) || Promise.resolve()).then(() => run(job)));
}

/** Starts sending `files`. @param {Element} root @param {File[]} files */
function start(root, files) {
  for (const file of files) {
    const why = check(root, file);
    const job = add(root, file, "queued");
    if (why) fail(job, why);
    else enqueue(job);
  }
}

/** Lists what the field holds, for a form that is sent by hand. @param {Element} root */
function chosen(root) {
  for (const li of all(root, "[data-state=chosen]")) li.remove();
  for (const file of field(root).files || []) add(root, file, "chosen");
}

/** Files arrive, chosen in the field or dropped. @param {Element} root @param {FileList | File[]} list @param {boolean} native */
function take(root, list, native) {
  const input = field(root);
  if (!input || has(root, "data-disabled")) return;
  let files = [...list];
  if (!has(input, "multiple")) files = files.slice(0, 1);
  if (auto(root)) {
    start(root, files);
    input.value = "";
    return;
  }
  if (!native) {
    const kept = new DataTransfer();
    for (const file of files) kept.items.add(file);
    input.files = kept.files;
  }
  chosen(root);
}

/** Takes a file out: stops it, or forgets it. @param {Job} job */
function leave(job) {
  const root = job.root, input = field(root);
  job.gone = true;
  job.x?.abort();
  if (job.id) req({}, "DELETE", (attr(root, "data-chunks") || "") + "/" + job.id, "", "").catch(() => {});
  if (job.state === "chosen") {
    const kept = new DataTransfer();
    for (const file of input.files || []) if (file !== job.file) kept.items.add(file);
    input.files = kept.files;
  }
  // A file a form's upload sent is no longer the form's.
  const key = keyField(root);
  if (key && job.state === "done") key.value = "";
  job.li.remove();
  needs(root);
  tell(root, say(root, "removed", { name: job.file.name }));
  input.focus();
}

on(document, "change", (e) => {
  const input = closest(e.target, ROOT + " input[type=file]");
  const root = input && closest(input, ROOT);
  if (root && has(root, "data-ready")) take(root, /** @type {HTMLInputElement} */ (input).files || [], true);
});

// A form sent by hand sends its files one by one, with progress. This runs before the core
// runtime's own submit handler, which then leaves a prevented submit alone.
on(document, "submit", (e) => {
  const root = closest(e.target, ROOT);
  if (!root || !has(root, "data-ready") || !attr(root, "action") || has(root, "data-disabled")) return;
  const input = field(root);
  const files = [...(input.files || [])];
  if (!files.length) return;
  e.preventDefault();
  // The field is emptied, so the rows of the chosen files give way to the rows that send them.
  input.value = "";
  chosen(root);
  start(root, files);
}, true);

// A form of the page's waits while one of its uploads is still sending: its key isn't there yet.
// (An upload in a form of the page's sends only when it has a `data-send` route.)
on(document, "submit", (e) => {
  const form = /** @type {Element} */ (e.target);
  for (const root of all(form, ROOT)) {
    const row = busy(root);
    const job = row && jobs.get(row);
    if (job) {
      e.preventDefault();
      tell(root, say(root, "sending", { name: job.file.name }));
      return;
    }
  }
}, true);

on(document, "click", (e) => {
  const button = closest(e.target, "[data-ui-cancel],[data-ui-retry],[data-ui-remove]");
  const job = button && jobs.get(/** @type {Element} */ (closest(button, ROW)));
  if (!button || !job) return;
  e.preventDefault();
  if (has(button, "data-ui-retry")) {
    job.gone = false;
    job.id = undefined;
    enqueue(job);
  } else leave(job);
});

on(document, "keydown", (e) => {
  if (e.key !== "Delete" && e.key !== "Backspace") return;
  const row = closest(e.target, ROW);
  const job = row && closest(e.target, "button") && jobs.get(row);
  if (job) {
    e.preventDefault();
    leave(job);
  }
});

// Dropping: files over the area are taken, and a file dropped anywhere else never takes the
// page away from where it is.
/** @param {DragEvent} e */
const dragging = (e) => !!e.dataTransfer && [...e.dataTransfer.types].includes("Files");
on(document, "dragenter", (e) => {
  const root = dragging(e) && closest(e.target, ROOT);
  if (root) set(root, "data-drag", "");
});
on(document, "dragover", (e) => {
  if (!dragging(e)) return;
  e.preventDefault();
  if (!closest(e.target, ROOT) && e.dataTransfer) e.dataTransfer.dropEffect = "none";
});
on(document, "dragleave", (e) => {
  const root = closest(e.target, ROOT);
  if (root && !holds(root, e.relatedTarget)) unset(root, "data-drag");
});
on(document, "drop", (e) => {
  if (!dragging(e)) return;
  e.preventDefault();
  const root = closest(e.target, ROOT);
  if (!root) return;
  unset(root, "data-drag");
  if (has(root, "data-ready")) take(root, e.dataTransfer?.files || [], false);
});

// The area is ready: the native control is laid over it, and the list takes over from its text.
function ready() {
  for (const root of every(ROOT)) set(root, "data-ready", "");
}
ready();
on(document, "ui:swap", ready);
