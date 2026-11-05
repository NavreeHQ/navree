// The upload component in real browsers, against the proof app's /upload: a native file input in
// a multipart form, the module's drop area and list, progress, stopping and trying again, large
// files in chunks, what the server refuses, JavaScript off, keys, right to left, hostile names and
// words, and axe in dark, light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/upload.test.mjs

import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { test } from "node:test";
import { BASE, axe, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };
const PNG_HEAD = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52]);
const EXE_HEAD = Buffer.from("MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0", "latin1");
const PDF_HEAD = Buffer.from("%PDF-1.7\n", "latin1");

const png = (name, size = 4096) => ({ name, mimeType: "image/png", buffer: Buffer.concat([PNG_HEAD, Buffer.alloc(Math.max(0, size - PNG_HEAD.length), 7)]) });
const pdf = (name, size = 4096) => ({ name, mimeType: "application/pdf", buffer: Buffer.concat([PDF_HEAD, Buffer.alloc(Math.max(0, size - PDF_HEAD.length), 9)]) });
const exe = (name, size = 4096) => ({ name, mimeType: "image/png", buffer: Buffer.concat([EXE_HEAD, Buffer.alloc(Math.max(0, size - EXE_HEAD.length), 1)]) });

/** A form body by hand, for posts a browser would never send: `[field, file name, bytes]`. */
function multipart(parts) {
  const boundary = "----navree-e2e-" + randomBytes(6).toString("hex");
  const chunks = [];
  for (const [field, name, bytes] of parts) {
    chunks.push(Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="${field}"; filename="${name}"\r\nContent-Type: application/octet-stream\r\n\r\n`), bytes, Buffer.from("\r\n"));
  }
  chunks.push(Buffer.from(`--${boundary}--\r\n`));
  return { data: Buffer.concat(chunks), headers: { "content-type": `multipart/form-data; boundary=${boundary}`, accept: "application/json" } };
}

inBrowsers(({ open, browserName }) => {
  const reset = (page) => page.request.post(BASE + "/upload/reset");
  // The page is drawn after the server forgets its files, so no row is left from the test before.
  const fresh = (options = {}) => open("/upload", { ...options, before: reset });
  const stored = async (page) => (await page.request.get(BASE + "/upload/stored.json")).json();
  const row = (page, id, state = "") => page.locator(`#${id} [data-slot=upload-item]${state ? `[data-state=${state}]` : ""}`);
  const said = (page, id) => page.locator(`#${id} [data-slot=upload-status]`).innerText();

  // UI-GENUI-021
  test("an upload is a multipart form around a native file input that the label names and the limits describe", async () => {
    const { page, close } = await fresh();
    const form = page.locator("#up-photo");
    assert.equal(await form.evaluate((e) => [e.tagName, e.method, e.enctype, e.getAttribute("action")]).then((a) => a.join(" ")), "FORM post multipart/form-data /upload/photo");
    const input = page.locator("#up-photo-input");
    assert.equal(await input.getAttribute("type"), "file");
    assert.equal(await input.getAttribute("accept"), "image/png,image/jpeg");
    assert.equal(await input.evaluate((e) => e.multiple), false);
    const name = await input.evaluate((e) => document.getElementById(e.getAttribute("aria-labelledby")).textContent);
    assert.equal(name.trim(), "Profile photo");
    const described = await input.evaluate((e) => e.getAttribute("aria-describedby").split(" ").map((id) => document.getElementById(id)?.textContent.trim()));
    assert.equal(described[0], "PNG or JPG, up to 1 MB");
    assert.equal(await page.locator("#up-photo [data-slot=upload-status]").getAttribute("role"), "status");
    // The module is ready: the native control lies over the drop area and takes the click.
    await page.waitForSelector("#up-photo[data-ready]");
    const covers = await page.evaluate(() => {
      // The input fills the area inside the zone's border.
      const zone = document.querySelector("#up-photo [data-slot=upload-zone]");
      const box = document.getElementById("up-photo-input").getBoundingClientRect();
      return Math.abs(zone.clientWidth - box.width) < 2 && Math.abs(zone.clientHeight - box.height) < 2;
    });
    assert.ok(covers, "the input covers the whole drop area");
    assert.equal(await page.locator("#up-photo [data-slot=upload-actions]").isVisible(), false, "no Send button while files go at once");
    await close();
  });

  test("a file chosen is sent at once, listed as sent, announced, and kept by the server", async () => {
    const { page, close } = await fresh();
    await page.locator("#up-photo-input").setInputFiles(png("Simran-Kaur.png", 3000));
    await row(page, "up-photo", "done").waitFor();
    assert.equal((await row(page, "up-photo").locator("[data-slot=attachment-title]").innerText()).trim(), "Simran-Kaur.png");
    assert.match(await row(page, "up-photo").locator("[data-slot=attachment-description]").innerText(), /2\.9 KB · Sent/);
    assert.match(await said(page, "up-photo"), /Simran-Kaur\.png · Sent/);
    const files = await stored(page);
    assert.deepEqual(files.map((f) => [f.name, f.mime, f.size]), [["Simran-Kaur.png", "image/png", "3000"]]);
    // The native control is empty again, so the same file can be chosen twice.
    assert.equal(await page.locator("#up-photo-input").evaluate((e) => e.files.length), 0);
    await shot(page, `${browserName}-upload-done`, "#photo");
    await close();
  });

  test("the server reads the kind from the bytes: a program renamed to a picture is refused in words", async () => {
    const { page, close } = await fresh();
    await page.locator("#up-photo-input").setInputFiles(exe("photo.png"));
    await row(page, "up-photo", "failed").waitFor();
    assert.equal((await row(page, "up-photo").locator("[data-slot=attachment-description]").innerText()).trim(), "photo.png is not an accepted kind of file. Accepted: PNG or JPG.");
    assert.match(await said(page, "up-photo"), /not an accepted kind of file/);
    assert.deepEqual(await stored(page), []);
    // A failed file can be tried again or taken out.
    assert.ok(await row(page, "up-photo").locator("[data-ui-retry]").isVisible());
    assert.ok(await row(page, "up-photo").locator("[data-ui-remove]").isVisible());
    assert.equal(await row(page, "up-photo").locator("[data-ui-cancel]").isVisible(), false);
    await close();
  });

  test("a file over the limit is refused by the page without a request, and by the server when the page is skipped", async () => {
    const { page, close } = await fresh();
    const posts = [];
    page.on("request", (request) => request.method() === "POST" && posts.push(new URL(request.url()).pathname));
    await page.locator("#up-photo-input").setInputFiles(png("big.png", 2 * 1024 * 1024));
    await row(page, "up-photo", "failed").waitFor();
    assert.equal((await row(page, "up-photo").locator("[data-slot=attachment-description]").innerText()).trim(), "big.png is larger than 1 MB.");
    assert.deepEqual(posts.filter((path) => path === "/upload/photo"), [], "nothing was sent");
    // A little over the limit: the server counts the file as it streams in and refuses it by name.
    const over = multipart([["photo", "big.png", png("x", 1024 * 1024 + 1000).buffer]]);
    const answer = await page.request.post(BASE + "/upload/photo", { data: over.data, headers: over.headers });
    assert.equal(answer.status(), 413);
    const body = await answer.json();
    assert.equal(body.refused[0].code, "too_large");
    assert.equal(body.refused[0].message, "big.png is larger than 1 MB.");
    // Far over: the route's own limit stops the whole request.
    const far = multipart([["photo", "huge.png", png("x", 3 * 1024 * 1024).buffer]]);
    assert.equal((await page.request.post(BASE + "/upload/photo", { data: far.data, headers: far.headers })).status(), 413);
    assert.deepEqual(await stored(page), []);
    await close();
  });

  test("an empty file is refused in words", async () => {
    const { page, close } = await fresh();
    await page.locator("#up-photo-input").setInputFiles({ name: "blank.png", mimeType: "image/png", buffer: Buffer.alloc(0) });
    await row(page, "up-photo", "failed").waitFor();
    assert.equal((await row(page, "up-photo").locator("[data-slot=attachment-description]").innerText()).trim(), "blank.png is empty.");
    await close();
  });

  test("a file dropped on the area is sent, and the area lights up while a file is over it", async () => {
    const { page, close } = await fresh();
    await page.waitForSelector("#up-photo[data-ready]");
    const zone = page.locator("#up-photo [data-slot=upload-zone]");
    const bytes = [...png("x", 2000).buffer];
    const data = await page.evaluateHandle((raw) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File([new Uint8Array(raw)], "Meera-Joshi.png", { type: "image/png" }));
      return transfer;
    }, bytes);
    await zone.dispatchEvent("dragenter", { dataTransfer: data });
    assert.equal(await page.locator("#up-photo").getAttribute("data-drag"), "");
    await zone.dispatchEvent("drop", { dataTransfer: data });
    assert.equal(await page.locator("#up-photo").getAttribute("data-drag"), null);
    await row(page, "up-photo", "done").waitFor();
    assert.deepEqual((await stored(page)).map((f) => f.name), ["Meera-Joshi.png"]);
    await close();
  });

  test("a file dropped anywhere else never takes the page away, and a drop that holds no file is ignored", async () => {
    const { page, close } = await open("/upload");
    await page.waitForSelector("#up-photo[data-ready]");
    const result = await page.evaluate(() => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(["hello"], "hello.txt", { type: "text/plain" }));
      const outside = document.querySelector("h1, h2");
      const make = (type) => new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: transfer });
      const over = make("dragover");
      outside.dispatchEvent(over);
      const drop = make("drop");
      outside.dispatchEvent(drop);
      const text = new DataTransfer();
      text.setData("text/plain", "words");
      const zone = document.querySelector("#up-photo [data-slot=upload-zone]");
      zone.dispatchEvent(new DragEvent("dragenter", { bubbles: true, dataTransfer: text }));
      return { overPrevented: over.defaultPrevented, dropPrevented: drop.defaultPrevented, effect: transfer.dropEffect, lit: document.getElementById("up-photo").hasAttribute("data-drag"), here: location.pathname };
    });
    assert.equal(result.overPrevented, true);
    assert.equal(result.dropPrevented, true);
    assert.equal(result.effect, "none");
    assert.equal(result.lit, false, "text is not a file");
    assert.equal(result.here, "/upload");
    await close();
  });

  test("a file on its way shows progress, can be stopped, and the list says so", async () => {
    const { page, close } = await fresh();
    await page.locator("#up-slow-input").setInputFiles({ name: "rates.csv", mimeType: "text/csv", buffer: Buffer.from("शहर,दर\nਲੁਧਿਆਣਾ,120\n") });
    const sending = row(page, "up-slow", "sending");
    await sending.waitFor();
    assert.ok(await sending.locator("progress").isVisible(), "the progress bar shows");
    assert.equal(await sending.locator("progress").getAttribute("aria-label"), "Sending rates.csv");
    assert.ok(await sending.locator("[data-ui-cancel]").isVisible());
    assert.equal(await sending.locator("[data-ui-retry]").isVisible(), false);
    assert.match(await said(page, "up-slow"), /Sending rates\.csv/);
    await sending.locator("[data-ui-cancel]").click();
    assert.equal(await row(page, "up-slow").count(), 0, "the row is gone");
    assert.match(await said(page, "up-slow"), /rates\.csv was removed/);
    assert.equal(await page.evaluate(() => document.activeElement?.id), "up-slow-input", "focus goes back to the control");
    await close();
  });

  test("a file that could not be sent says so, and Try again sends it", async () => {
    const { page, close } = await fresh();
    let tries = 0;
    await page.route("**/upload/photo", (route) => (tries++ === 0 ? route.abort("connectionreset") : route.continue()));
    await page.locator("#up-photo-input").setInputFiles(png("retry.png"));
    await row(page, "up-photo", "failed").waitFor();
    assert.match(await row(page, "up-photo").locator("[data-slot=attachment-description]").innerText(), /retry\.png could not be sent\. Check your connection/);
    await row(page, "up-photo").locator("[data-ui-retry]").click();
    await row(page, "up-photo", "done").waitFor();
    assert.deepEqual((await stored(page)).map((f) => f.name), ["retry.png"]);
    await page.unroute("**/upload/photo");
    await close();
  });

  test("a large file goes in parts, each tried again, and arrives whole", async () => {
    const { page, close } = await fresh();
    const body = Buffer.concat([PDF_HEAD, randomBytes(12 * 1024 * 1024)]);
    const calls = [];
    page.on("request", (request) => {
      const path = new URL(request.url()).pathname;
      if (path.startsWith("/upload/chunks")) calls.push(`${request.method()} ${path.replace(/[0-9a-f]{32}/, "{id}")}`);
    });
    let third = 0;
    await page.route("**/upload/chunks/*/3", (route) => (third++ === 0 ? route.abort("connectionreset") : route.continue()));
    await page.locator("#up-docs-input").setInputFiles({ name: "GST-returns-2026.pdf", mimeType: "application/pdf", buffer: body });
    await row(page, "up-docs", "done").waitFor({ timeout: 120000 });
    assert.equal(calls.filter((c) => c === "POST /upload/chunks").length, 1, "one start");
    assert.equal(calls.filter((c) => c.startsWith("PUT ")).length, 14, "twelve parts of 1 MB, the short last one, and the one that was tried again");
    assert.equal(calls.filter((c) => c === "POST /upload/chunks/{id}/finish").length, 1, "one finish");
    assert.deepEqual((await stored(page)).map((f) => [f.name, f.mime, f.size]), [["GST-returns-2026.pdf", "application/pdf", String(body.length)]]);
    // The region that lists the files was fetched again when the last file was over.
    await page.waitForFunction(() => document.getElementById("up-stored")?.textContent.includes("GST-returns-2026.pdf"));
    await page.unroute("**/upload/chunks/*/3");
    await close();
  });

  test("a large file of a kind that is not accepted is refused when it is put together, and leaves nothing", async () => {
    const { page, close } = await fresh();
    await page.locator("#up-docs-input").setInputFiles({ name: "evil.pdf", mimeType: "application/pdf", buffer: Buffer.concat([EXE_HEAD, randomBytes(5 * 1024 * 1024)]) });
    await row(page, "up-docs", "failed").waitFor({ timeout: 60000 });
    assert.match(await row(page, "up-docs").locator("[data-slot=attachment-description]").innerText(), /evil\.pdf is not an accepted kind of file/);
    assert.deepEqual(await stored(page), []);
    await close();
  });

  test("the server refuses a made-up upload id, a file larger than the limit at the start, and a count past the room", async () => {
    const { page, close } = await fresh();
    const forged = await page.request.put(BASE + "/upload/chunks/0123456789abcdef0123456789abcdef/0", { data: Buffer.alloc(10), headers: { accept: "application/json" } });
    assert.equal(forged.status(), 404);
    assert.equal((await forged.json()).refused[0].code, "unknown");
    const big = await page.request.post(BASE + "/upload/chunks", { data: { name: "huge.pdf", size: 30 * 1024 * 1024 } });
    assert.equal(big.status(), 413);
    assert.equal((await big.json()).message, "huge.pdf is larger than 20 MB.");
    const six = multipart(Array.from({ length: 6 }, (_, n) => ["docs", `page-${n + 1}.pdf`, pdf("x", 600).buffer]));
    const answer = await page.request.post(BASE + "/upload/docs", { data: six.data, headers: six.headers });
    const body = await answer.json();
    assert.equal(answer.status(), 200, "five were kept");
    assert.equal(body.stored.length, 5);
    assert.deepEqual(body.refused.map((r) => [r.name, r.code, r.message]), [["page-6.pdf", "too_many", "You can send up to 5 files."]]);
    assert.equal((await stored(page)).length, 5);
    await close();
  });

  test("hostile names stay text on the page, in the list the server drew and in the one the module draws", async () => {
    const { page, close } = await fresh();
    await page.evaluate(() => { window.__xss = 0; });
    const drawn = await page.locator("#up-hostile [data-slot=attachment-title]").evaluateAll((els) => els.map((e) => [e.textContent, e.querySelector("img") === null]));
    assert.deepEqual(drawn[0], ["<img src=x onerror=alert(1)>.pdf", true]);
    assert.equal(drawn[1][0], "ਗੁਰਪ੍ਰੀਤ-ਸਿੰਘ-ਆਧਾਰ.pdf");
    const evil = "<img src=x onerror=window.__xss=1>.png";
    await page.locator("#up-photo-input").setInputFiles({ name: evil, mimeType: "image/png", buffer: png("x").buffer });
    await row(page, "up-photo", "done").waitFor();
    assert.equal(await row(page, "up-photo").locator("[data-slot=attachment-title]").textContent(), evil);
    assert.equal(await page.locator("#up-photo img").count(), 0);
    assert.equal(await page.evaluate(() => window.__xss), 0);
    await close();
  });

  test("words from a hostile server are written as text", async () => {
    const { page, close } = await fresh();
    const evil = "<img src=x onerror=window.__xss=1><b>bold</b>";
    await page.route("**/upload/photo", (route) => route.fulfill({ status: 422, contentType: "application/json", body: JSON.stringify({ refused: [{ message: evil }] }) }));
    await page.locator("#up-photo-input").setInputFiles(png("a.png"));
    await row(page, "up-photo", "failed").waitFor();
    assert.equal(await row(page, "up-photo").locator("[data-slot=attachment-description]").textContent(), evil);
    assert.equal(await page.locator("#up-photo img, #up-photo b").count(), 0);
    await page.unroute("**/upload/photo");
    await close();
  });

  test("a form sent by hand lists the files and sends them one by one with the Send button", async () => {
    const { page, close } = await fresh();
    await page.waitForSelector("#up-manual[data-ready]");
    assert.ok(await page.locator("#up-manual [data-slot=upload-actions] button").isVisible(), "Send shows");
    await page.locator("#up-manual-input").setInputFiles({ name: "prices.csv", mimeType: "text/csv", buffer: Buffer.from("naam,keemat\nghee,650\n") });
    await row(page, "up-manual", "chosen").waitFor();
    assert.equal((await stored(page)).length, 0, "nothing goes until Send");
    await page.locator("#up-manual [data-slot=upload-actions] button").click();
    await row(page, "up-manual", "done").waitFor();
    assert.equal(page.url().endsWith("/upload"), true, "the page stayed where it was");
    assert.deepEqual((await stored(page)).map((f) => f.name), ["prices.csv"]);
    await close();
  });

  test("a field in a form lists the chosen files, takes one out, and sends the rest with the form", async () => {
    const { page, close } = await fresh();
    await page.waitForSelector("#up-field[data-ready]");
    await page.locator("#up-field-input").setInputFiles([pdf("Aadhaar.pdf"), pdf("PAN.pdf")]);
    assert.equal(await row(page, "up-field", "chosen").count(), 2);
    assert.equal(await page.locator("#up-field-input").evaluate((e) => e.multiple), true);
    await row(page, "up-field", "chosen").first().locator("[data-ui-remove]").click();
    assert.equal(await row(page, "up-field", "chosen").count(), 1);
    assert.deepEqual(await page.locator("#up-field-input").evaluate((e) => [...e.files].map((f) => f.name)), ["PAN.pdf"]);
    const sent = page.waitForRequest((request) => new URL(request.url()).pathname === "/upload/docs" && request.method() === "POST");
    await page.locator("#up-form button[type=submit]").click();
    const request = await sent;
    // Chromium reports no body for a post that holds a file; the files the server kept, below,
    // say the same in every browser.
    const post = request.postDataBuffer()?.toString("latin1");
    if (post !== undefined) assert.ok(post.includes("PAN.pdf") && !post.includes("Aadhaar.pdf"), "only the file left in the list was sent");
    await page.waitForURL(/\/upload#documents/);
    assert.deepEqual((await stored(page)).map((f) => f.name), ["PAN.pdf"]);
    await close();
  });

  test("keys: Delete on the button of a listed file takes it out and says so, and focus goes back to the control", async () => {
    const { page, close } = await fresh();
    await page.waitForSelector("#up-field[data-ready]");
    await page.locator("#up-field-input").setInputFiles([pdf("Salary-slip.pdf")]);
    const remove = row(page, "up-field", "chosen").locator("[data-ui-remove]");
    await remove.focus();
    await page.keyboard.press("Delete");
    assert.equal(await row(page, "up-field").count(), 0);
    assert.match(await said(page, "up-field"), /Salary-slip\.pdf was removed/);
    assert.equal(await page.evaluate(() => document.activeElement?.id), "up-field-input");
    await close();
  });

  test("keyboard only: the control takes focus, shows a ring on the area, and the chooser opens from it", async () => {
    const { page, close } = await open("/upload");
    await page.keyboard.press("Tab");
    await page.locator("#up-photo-input").focus();
    const ring = await page.locator("#up-photo [data-slot=upload-zone]").evaluate((e) => getComputedStyle(e).outlineStyle);
    assert.notEqual(ring, "none", "the ring shows on the area");
    const chooser = page.waitForEvent("filechooser");
    await page.keyboard.press("Enter");
    const opened = await chooser;
    assert.equal(opened.isMultiple(), false);
    await close();
  });

  test("the states: disabled can't be chosen, an error is tied to the control and read out", async () => {
    const { page, close } = await open("/upload");
    assert.equal(await page.locator("#up-disabled-input").isDisabled(), true);
    const error = page.locator("#up-error-input");
    assert.equal(await error.getAttribute("aria-invalid"), "true");
    assert.equal(await error.getAttribute("required"), "");
    const text = await error.evaluate((e) => e.getAttribute("aria-describedby").split(" ").map((id) => document.getElementById(id)?.textContent.trim()).join("|"));
    assert.match(text, /Choose a photo under 1 MB\./);
    await close();
  });

  test("right to left: the list and its buttons follow the page's direction", async () => {
    const { page, close } = await fresh();
    await page.waitForSelector("#up-rtl-field[data-ready]");
    await page.locator("#up-rtl-field-input").setInputFiles(exe("ملف.png"));
    await row(page, "up-rtl-field", "failed").waitFor();
    const places = await page.evaluate(() => {
      const li = document.querySelector("#up-rtl-field [data-slot=upload-item]");
      const media = li.querySelector("[data-slot=attachment]").getBoundingClientRect();
      const actions = li.querySelector("[data-slot=upload-row-actions]").getBoundingClientRect();
      return { attachmentLeft: media.left, actionsLeft: actions.left, dir: getComputedStyle(li).direction };
    });
    assert.equal(places.dir, "rtl");
    assert.ok(places.actionsLeft < places.attachmentLeft + 1, "the buttons sit at the end of the line, which is the left");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
    await close();
  });

  test("no sideways scroll at 390 px, even for a very long name", async () => {
    const { page, close } = await fresh({ viewport: PHONE });
    await page.waitForSelector("#up-photo[data-ready]");
    await page.locator("#up-photo-input").setInputFiles(exe(`${"Gurpreet-Singh-".repeat(14)}.png`));
    await row(page, "up-photo", "failed").waitFor();
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "the page does not grow sideways");
    await shot(page, `${browserName}-upload-390`, "#photo");
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in any state of the upload (${label})`, async () => {
      // A file the server already has, which the page draws with its Remove button.
      const keep = async (page) => {
        await reset(page);
        const kept = multipart([["docs", "Voter-ID-Harpreet-Kaur.pdf", pdf("x").buffer]]);
        await page.request.post(BASE + "/upload/docs", { data: kept.data, headers: kept.headers });
      };
      const { page, close } = await open("/upload", { ...options, before: keep });
      await page.waitForSelector("#up-field[data-ready]");
      // Every state on show at once: a failed file, a chosen one, a sent one, and a stored one.
      await page.locator("#up-photo-input").setInputFiles(exe("not-a-photo.png"));
      await row(page, "up-photo", "failed").waitFor();
      await page.locator("#up-field-input").setInputFiles([pdf("Salary-slip-October.pdf")]);
      await page.locator("#up-docs-input").setInputFiles(pdf("Aadhaar-Gurpreet-Singh.pdf"));
      await row(page, "up-docs", "done").filter({ hasText: "Aadhaar-Gurpreet-Singh.pdf" }).waitFor();
      for (const id of ["photo", "documents", "manual", "field", "states", "rtl"]) {
        assert.deepEqual(await axe(page, `#${id}`), [], id);
      }
      await close();
    });
  }

  test("JavaScript off: choose a file, press Send, and the page comes back with the file listed and a toast", async () => {
    const { page, close } = await fresh({ js: false });
    assert.equal(await page.locator("#up-photo[data-ready]").count(), 0);
    assert.ok(await page.locator("#up-photo [data-slot=upload-actions] button").isVisible(), "Send shows");
    assert.ok(await page.locator("#up-photo-input").isVisible(), "the native control shows");
    await page.locator("#up-photo-input").setInputFiles(png("Navdeep-Grewal.png"));
    await Promise.all([page.waitForURL(/\/upload#photo/), page.locator("#up-photo [data-slot=upload-actions] button").click()]);
    assert.deepEqual((await stored(page)).map((f) => f.name), ["Navdeep-Grewal.png"]);
    assert.match(await page.locator("body").innerText(), /1 file sent\./);
    assert.equal(await page.locator("#up-docs [data-slot=upload-item] [data-slot=attachment-title]").first().innerText(), "Navdeep-Grewal.png");
    await close();
  });

  test("JavaScript off: Send with no file says to choose one, and a stored file removes itself with a post", async () => {
    const { page, close } = await fresh({ js: false });
    await Promise.all([page.waitForURL(/\/upload#photo/), page.locator("#up-photo [data-slot=upload-actions] button").click()]);
    assert.match(await page.locator("body").innerText(), /Choose a file to send\./);
    await page.locator("#up-photo-input").setInputFiles(png("Rajveer-Brar.png"));
    await Promise.all([page.waitForURL(/\/upload#photo/), page.locator("#up-photo [data-slot=upload-actions] button").click()]);
    await Promise.all([page.waitForURL(/\/upload#documents/), page.locator("#up-docs [data-slot=upload-item] button[formaction^='/upload/remove/']").first().click()]);
    assert.deepEqual(await stored(page), []);
    await close();
  });

  test("JavaScript off: a file the server refuses is told in words after the redirect", async () => {
    const { page, close } = await fresh({ js: false });
    await page.locator("#up-photo-input").setInputFiles(exe("photo.png"));
    await Promise.all([page.waitForURL(/\/upload#photo/), page.locator("#up-photo [data-slot=upload-actions] button").click()]);
    assert.match(await page.locator("body").innerText(), /photo\.png is not an accepted kind of file/);
    assert.deepEqual(await stored(page), []);
    await close();
  });
});
