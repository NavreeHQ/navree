// The one-time code field in real browsers, against the proof app's /otp: the attributes a
// phone and a screen reader need, typing, pasting, the caret, completion and the form sent
// when the code is whole, boxes that line up with their characters (measured on the
// pixels), groups, twelve boxes on a phone, right to left, JavaScript off, the server's
// reading of a posted code, and axe in dark and light at 1440 and 390 px.
//
// UI-GENUI-020
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/otp.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, axe, inBrowsers, shot } from "./support/flow.mjs";
import { decodePng } from "./support/png.mjs";

const PHONE = { width: 390, height: 844 };

/** Types `text` into the field with this id, one key at a time. */
async function type(page, id, text) {
  await page.locator(`#${id}`).click();
  await page.keyboard.type(text);
}

/** Pastes `text` into the field with this id, as a browser does; false when the engine will not give a made-up paste its text. */
async function paste(page, id, text) {
  return page.evaluate(
    ([id, text]) => {
      const field = document.getElementById(id);
      field.focus();
      const data = new DataTransfer();
      data.setData("text/plain", text);
      const event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true });
      if (!event.clipboardData) return false;
      field.dispatchEvent(event);
      return true;
    },
    [id, text],
  );
}

const value = (page, id) => page.locator(`#${id}`).inputValue();

/** The box round a field. */
const box = (page, id) => page.locator(`[data-slot=input-otp]:has(#${id})`);

/**
 * Where the ink of each character sits against the box it is in, read from a screenshot of the
 * field typed full of zeros: for every slot, how far the middle of its ink is from the middle
 * of the slot, and how much clear space it leaves on each side.
 */
async function ink(page, id, length) {
  await page.locator(`#${id}`).fill("");
  await page.locator(`#${id}`).click();
  await page.keyboard.type("0".repeat(length));
  await page.evaluate(() => document.activeElement?.blur());
  const wrapper = box(page, id);
  const info = await wrapper.evaluate((el) => {
    const input = el.querySelector("input");
    const bounds = el.getBoundingClientRect();
    return {
      width: bounds.width,
      colour: getComputedStyle(input).color,
      scroll: input.scrollLeft,
      left: Math.round((bounds.width - el.clientWidth) / 2),
    };
  });
  const picture = decodePng(await wrapper.screenshot());
  const read = (x, y) => {
    const at = (y * picture.width + x) * picture.channels;
    const [r, g, b] = [picture.data[at], picture.data[at + 1], picture.data[at + 2]];
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const background = read(3, 3);
  const [fr, fg, fb] = info.colour.match(/\d+/g).map(Number);
  const foreground = 0.2126 * fr + 0.7152 * fg + 0.0722 * fb;
  const threshold = background + 0.6 * (foreground - background);
  const pitch = (info.width - 2 * info.left) / length;
  const slots = [];
  for (let k = 0; k < length; k += 1) {
    const from = info.left + k * pitch;
    const to = from + pitch;
    let first = Infinity;
    let last = -Infinity;
    for (let x = Math.ceil(from) + 2; x < Math.floor(to) - 2; x += 1) {
      for (let y = 4; y < picture.height - 4; y += 1) {
        if (read(x, y) > threshold) {
          first = Math.min(first, x);
          last = Math.max(last, x + 1);
          break;
        }
      }
    }
    slots.push({ off: (first + last) / 2 - (from + to) / 2, before: first - from, after: to - last, found: first !== Infinity });
  }
  return { slots, scroll: info.scroll, pitch };
}

inBrowsers(({ open, browserName }) => {
  test("a code field is one text input with what a phone and a screen reader need", async () => {
    const { page, close } = await open("/otp");
    const field = page.locator("#otp-six");
    assert.equal(await page.locator("[data-slot=input-otp]:has(#otp-six) input").count(), 1);
    const read = await field.evaluate((e) => ({
      type: e.type,
      // The attribute: Firefox's property is empty for a token it does not fill itself.
      autocomplete: e.getAttribute("autocomplete"),
      mode: e.inputMode,
      max: e.maxLength,
      min: e.minLength,
      pattern: e.pattern,
      spell: e.spellcheck,
      name: e.name,
      label: e.labels[0]?.textContent,
    }));
    assert.deepEqual(read, {
      type: "text",
      autocomplete: "one-time-code",
      mode: "numeric",
      max: 6,
      min: 6,
      pattern: "[0-9]{6}",
      spell: false,
      name: "code",
      label: "Verification code",
    });
    assert.equal(await page.getByLabel("Verification code").count(), 1, "the label names the field");
    const letters = await page.locator("#otp-voucher").evaluate((e) => [e.inputMode, e.pattern, e.autocapitalize]);
    assert.deepEqual(letters, ["text", "[0-9A-Za-z]{8}", "characters"]);
    await close();
  });

  test("typing fills the code in order and stops at its length", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-six", "4829157");
    assert.equal(await value(page, "otp-six"), "482915");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.scrollLeft), 0, "the text never scrolls");
    await close();
  });

  test("letters, spaces and dashes are dropped from a digits field as they are typed", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-six", "4a8 2-9x1 5");
    assert.equal(await value(page, "otp-six"), "482915");
    await close();
  });

  test("a code with letters takes capitals and drops the rest", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-voucher", "ab-12 cd#3é");
    assert.equal(await value(page, "otp-voucher"), "AB12CD3");
    await close();
  });

  test("a pasted code lands whole, with its spaces and dashes taken out", async (t) => {
    const { page, close } = await open("/otp");
    if (!(await paste(page, "otp-six", "482 915"))) {
      await close();
      return t.skip("this engine gives a made-up paste no text");
    }
    assert.equal(await value(page, "otp-six"), "482915", "the seventh character of the paste is not lost to the length limit");
    await page.locator("#otp-six").fill("");
    assert.equal(await paste(page, "otp-grouped", "  48-29-15\n"), true);
    assert.equal(await value(page, "otp-grouped"), "482915");
    // Arabic-Indic digits are not digits for a code: nothing lands.
    await paste(page, "otp-four", "١٢٣٤");
    assert.equal(await value(page, "otp-four"), "");
    await close();
  });

  test("Backspace takes the last character and a click puts the caret at the end", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-six", "4829");
    // A click in the first box would put the caret there; the next digit still goes on the end.
    const at = await box(page, "otp-six").boundingBox();
    await page.mouse.click(at.x + 12, at.y + at.height / 2);
    await page.keyboard.type("1");
    assert.equal(await value(page, "otp-six"), "48291");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Backspace");
    assert.equal(await value(page, "otp-six"), "482");
    await close();
  });

  test("the arrow keys still move the caret, so the code can be read back", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-six", "4829");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.selectionStart), 2);
    await page.keyboard.press("Home");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.selectionStart), 0);
    await close();
  });

  test("a whole code says so once, and the form is sent only when the field asks", async () => {
    const { page, close } = await open("/otp");
    await page.evaluate(() => {
      window.__changes = 0;
      document.getElementById("otp-six").addEventListener("change", () => (window.__changes += 1));
    });
    await type(page, "otp-six", "48291");
    assert.equal(await page.evaluate(() => window.__changes), 0);
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.hasAttribute("data-done")), false);
    await page.keyboard.type("5");
    assert.equal(await page.evaluate(() => window.__changes), 1);
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.hasAttribute("data-done")), true);
    assert.equal(new URL(page.url()).pathname, "/otp", "a field that did not ask does not send the form");
    // Taking one away and typing it again is a new completion.
    await page.keyboard.press("Backspace");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.hasAttribute("data-done")), false);
    await page.keyboard.type("5");
    assert.equal(await page.evaluate(() => window.__changes), 2);
    await close();
  });

  test("a field that asks has its form sent when the code is whole", async () => {
    const { page, close } = await open("/otp");
    await type(page, "otp-auto", "482915");
    await page.waitForURL("**/otp/echo");
    assert.match(await page.locator("#judged").innerText(), /auto: ok 482915/);
    await close();
  });

  test("a code the page drew whole says so when it is whole again, not before", async () => {
    const { page, close } = await open("/otp");
    const field = page.locator("#otp-prefilled");
    assert.equal(await field.inputValue(), "482915");
    await page.evaluate(() => {
      window.__changes = 0;
      document.getElementById("otp-prefilled").addEventListener("change", () => (window.__changes += 1));
    });
    await field.click();
    await page.keyboard.press("Backspace");
    assert.equal(await field.inputValue(), "48291");
    assert.equal(await page.evaluate(() => window.__changes), 0, "a code that is no longer whole says nothing");
    await page.keyboard.type("5");
    assert.equal(await page.evaluate(() => window.__changes), 1);
    await close();
  });

  for (const [label, viewport] of [["1440", undefined], ["390", PHONE]]) {
    test(`each character sits in the middle of its box at ${label} px`, async () => {
      const { page, close } = await open("/otp", { viewport });
      for (const [id, length] of [["otp-six", 6], ["otp-grouped", 6], ["otp-four", 4]]) {
        const read = await ink(page, id, length);
        assert.equal(read.scroll, 0, `${id}: the text did not scroll`);
        for (const [k, slot] of read.slots.entries()) {
          assert.ok(slot.found, `${id}: a character is drawn in box ${k + 1}`);
          assert.ok(Math.abs(slot.off) <= 1.5, `${id}: box ${k + 1} is off by ${slot.off}px`);
          assert.ok(slot.before >= 3 && slot.after >= 3, `${id}: box ${k + 1} has room round its character (${slot.before}, ${slot.after})`);
        }
      }
      await shot(page, `${browserName}-otp-${label}`, "#codes");
      await close();
    });
  }

  test("the boxes are as wide as the code needs: N boxes of one pitch", async () => {
    const { page, close } = await open("/otp");
    for (const [id, length] of [["otp-four", 4], ["otp-six", 6], ["otp-voucher", 8], ["otp-twelve", 12]]) {
      const read = await box(page, id).evaluate((el, n) => {
        const input = el.querySelector("input");
        const style = getComputedStyle(input);
        const probe = document.createElement("span");
        probe.style.position = "absolute";
        probe.style.visibility = "hidden";
        probe.style.whiteSpace = "pre";
        probe.style.fontFamily = style.fontFamily;
        probe.style.fontSize = style.fontSize;
        probe.style.fontWeight = style.fontWeight;
        probe.textContent = "0";
        document.body.append(probe);
        const glyph = probe.getBoundingClientRect().width;
        probe.remove();
        const pitch = (el.clientWidth) / n;
        return { glyph, spacing: parseFloat(style.letterSpacing), pad: parseFloat(style.paddingLeft), pitch };
      }, length);
      assert.ok(Math.abs(read.glyph + read.spacing - read.pitch) < 0.6, `${id}: a character and its spacing take one box (${read.glyph} + ${read.spacing} vs ${read.pitch})`);
      assert.ok(Math.abs(read.pad - read.spacing / 2) < 0.6, `${id}: the first character starts half a spacing in`);
    }
    await close();
  });

  test("a code in groups draws a heavier line, and the thin lines stay", async () => {
    const { page, close } = await open("/otp");
    const layers = (id) => box(page, id).evaluate((el) => getComputedStyle(el).backgroundImage.split("repeating-linear-gradient").length - 1);
    assert.equal(await layers("otp-grouped"), 2);
    assert.equal(await layers("otp-six"), 1);
    await close();
  });

  test("twelve boxes fit a phone with no sideways scroll", async () => {
    const { page, close } = await open("/otp", { viewport: PHONE });
    const right = await box(page, "otp-twelve").evaluate((el) => el.getBoundingClientRect().right);
    assert.ok(right <= 390, `the twelve boxes end at ${right}`);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    await type(page, "otp-twelve", "482915739104");
    assert.equal(await value(page, "otp-twelve"), "482915739104");
    assert.equal(await page.locator("#otp-twelve").evaluate((e) => e.scrollLeft), 0);
    await close();
  });

  test("states: disabled cannot be focused, read-only keeps its code, invalid is announced", async () => {
    const { page, close } = await open("/otp");
    assert.equal(await page.locator("#otp-disabled").isDisabled(), true);
    const readonly = page.locator("#otp-readonly");
    await readonly.click();
    await page.keyboard.type("999999");
    await page.keyboard.press("Backspace");
    assert.equal(await readonly.inputValue(), "482915");
    const invalid = await page.locator("#otp-invalid").evaluate((e) => ({
      invalid: e.getAttribute("aria-invalid"),
      says: document.getElementById(e.getAttribute("aria-describedby").split(" ").at(-1)).textContent,
    }));
    assert.equal(invalid.invalid, "true");
    assert.match(invalid.says, /expired/);
    await close();
  });

  test("a focused field shows a ring round its boxes", async () => {
    const { page, close } = await open("/otp");
    await page.keyboard.press("Tab");
    await page.locator("#otp-six").focus();
    const ring = await box(page, "otp-six").evaluate((el) => [getComputedStyle(el).outlineStyle, getComputedStyle(el).outlineWidth]);
    assert.deepEqual(ring, ["solid", "2px"]);
    await close();
  });

  test("in a right-to-left page the code still reads left to right and its boxes still line up", async () => {
    const { page, close } = await open("/otp");
    const direction = await box(page, "otp-rtl").evaluate((el) => [getComputedStyle(el).direction, getComputedStyle(el.querySelector("input")).direction, el.closest("[dir=rtl]") !== null]);
    assert.deepEqual(direction, ["ltr", "ltr", true]);
    await type(page, "otp-rtl", "123456");
    assert.equal(await value(page, "otp-rtl"), "123456");
    const read = await ink(page, "otp-rtl", 6);
    for (const [k, slot] of read.slots.entries()) assert.ok(Math.abs(slot.off) <= 1.5, `box ${k + 1} is off by ${slot.off}px`);
    await close();
  });

  test("with JavaScript off the field is a plain input that posts, and a short code is held back", async () => {
    const { page, close } = await open("/otp", { js: false });
    assert.equal(await page.locator("[data-slot=input-otp]:has(#otp-six)").count(), 1);
    await page.locator("#otp-six").fill("4829");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.checkValidity()), false, "four of six is not a code");
    await page.locator("#otp-six").fill("482 915");
    assert.equal(await page.locator("#otp-six").evaluate((e) => e.checkValidity()), false, "a space is not a digit to the pattern; the script, when there is one, takes it out");
    await page.locator("#otp-six").fill("482915");
    await page.locator("#otp-send").click();
    await page.waitForURL("**/otp/echo");
    assert.match(await page.locator("#judged").innerText(), /code: ok 482915/);
    await close();
  });

  test("the server reads a posted code under the field's rules", async () => {
    const { page, close } = await open("/otp");
    const judged = (body) =>
      page.evaluate(async (body) => {
        const reply = await fetch("/otp/echo", { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body });
        const html = await reply.text();
        return /<pre id="judged">([^<]*)<\/pre>/.exec(html)?.[1] ?? "";
      }, body);
    assert.match(await judged("code=482+915"), /code: ok 482915/);
    assert.match(await judged("code=%0A482-915%0A"), /code: ok 482915/);
    assert.match(await judged("code=48291"), /code: The code is not the right length\./);
    assert.match(await judged("code=%D9%A1%D9%A2%D9%A3%D9%A4%D9%A5%D9%A6"), /code: The code has a character it cannot have\./);
    assert.match(await judged("code=123%E2%80%8B456"), /code: The code has a character it cannot have\./);
    assert.match(await judged("code=%EF%BC%91%EF%BC%92%EF%BC%93%EF%BC%94%EF%BC%95%EF%BC%96"), /code: The code has a character it cannot have\./);
    assert.match(await judged("code=" + "1".repeat(5000)), /code: The code is not the right length\./);
    assert.match(await judged("voucher=ab-12+cd34"), /voucher: ok AB12CD34/);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in the code fields (${label})`, async () => {
      const { page, close } = await open("/otp", options);
      assert.deepEqual(await axe(page, "#codes"), []);
      await type(page, "otp-six", "4829");
      assert.deepEqual(await axe(page, "#codes"), []);
      await close();
    });
  }
});
