// The stepper and the slider in real browsers, against the proof app's /flow: states and words,
// the keys, the filled part, ranges and their gap, right to left, the form's post and what the
// server makes of it, JavaScript off, and axe in dark, light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/flow-slider.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, axe, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };

inBrowsers(({ open, browserName }) => {
  const read = (page, id) =>
    page.evaluate((i) => {
      const input = document.getElementById(i);
      const root = input.closest("[data-ui-slider]");
      return {
        value: input.value,
        text: input.getAttribute("aria-valuetext"),
        output: root.querySelector("output")?.textContent,
        fill: root.querySelector("[data-slot=slider-fill]").getAttribute("width"),
        x: root.querySelector("[data-slot=slider-fill]").getAttribute("x"),
      };
    }, id);

  const room = async (page, id) => {
    await page.locator(`#${id}`).scrollIntoViewIfNeeded();
  };

  test("the stepper says each state in words and marks the current step", async () => {
    const { page, close } = await open("/flow");
    const list = page.locator("#s-horizontal");
    assert.equal(await list.locator("[aria-current=step]").count(), 1);
    assert.deepEqual(
      await list.locator("[data-slot=step]").evaluateAll((els) => els.map((e) => e.getAttribute("data-state"))),
      ["done", "done", "error", "current", "disabled"],
    );
    assert.match(await list.evaluate((e) => e.textContent), /\(completed\)/);
    assert.match(await list.evaluate((e) => e.textContent), /\(needs attention\)/);
    assert.match(await list.evaluate((e) => e.textContent), /\(current step\)/);
    assert.match(await list.evaluate((e) => e.textContent), /\(not available yet\)/);
    const drawn = await list.locator("[data-ui-sr]").first().evaluate((e) => { const b = e.getBoundingClientRect(); return b.width <= 1 && b.height <= 1; });
    assert.ok(drawn, "the words for assistive technology take no room");
    // The current step and the disabled one are not links; the others are.
    assert.equal(await list.locator("a[href]").count(), 3);
    await close();
  });

  test("a stepper in a narrow box keeps the current step's name and writes the count, with no sideways scroll", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    const count = page.locator("#s-horizontal + [data-slot=stepper-count], #s-horizontal ~ [data-slot=stepper-count]").first();
    assert.equal((await count.innerText()).trim(), "Step 4 of 5");
    const visible = await page.locator("#s-horizontal [data-slot=step]").evaluateAll((steps) =>
      steps.map((step) => {
        const box = step.querySelector("[data-slot=step-text]").getBoundingClientRect();
        return box.width > 4;
      }),
    );
    assert.deepEqual(visible, [false, false, false, true, false]);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    await shot(page, `${browserName}-stepper-390`);
    await close();
  });

  test("a vertical stepper keeps every name, and a long one wraps", async () => {
    const { page, close } = await open("/flow", { viewport: PHONE });
    const widths = await page.locator("#s-vertical [data-slot=step-title]").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().width > 40));
    assert.deepEqual(widths, [true, true, true, true]);
    const overflow = await page.locator("#s-long").evaluate((e) => e.scrollWidth <= e.clientWidth + 1 && e.getBoundingClientRect().right <= window.innerWidth);
    assert.ok(overflow, "a long name wraps instead of widening the page");
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
  ]) {
    test(`axe finds nothing in the steppers, dark and light and on a phone (${label})`, async () => {
      const { page, close } = await open("/flow", options);
      assert.deepEqual(await axe(page, "#steppers"), []);
      await close();
    });
  }

  test("a slider is a native range input that carries the page's value, its text and the filled part", async () => {
    const { page, close } = await open("/flow");
    const kind = await page.locator("#sl-volume").evaluate((e) => [e.type, e.name, e.min, e.max, e.step]);
    assert.deepEqual(kind, ["range", "volume", "0", "100", "5"]);
    assert.deepEqual(await read(page, "sl-volume"), { value: "40", text: "40%", output: "40%", fill: "40%", x: "0%" });
    await close();
  });

  test("the keys move a slider by a step, to the ends, by a tenth of the track and by a larger step", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-volume");
    await page.focus("#sl-volume");
    await page.keyboard.press("ArrowRight");
    assert.deepEqual(await read(page, "sl-volume"), { value: "45", text: "45%", output: "45%", fill: "45%", x: "0%" });
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    assert.equal((await read(page, "sl-volume")).value, "35");
    await page.keyboard.press("Home");
    assert.deepEqual(await read(page, "sl-volume"), { value: "0", text: "0%", output: "0%", fill: "0%", x: "0%" });
    await page.keyboard.press("End");
    assert.equal((await read(page, "sl-volume")).output, "100%");
    await page.keyboard.press("PageDown");
    assert.equal((await read(page, "sl-volume")).value, "90");
    await page.keyboard.press("Shift+ArrowLeft");
    assert.equal((await read(page, "sl-volume")).value, "80");
    await page.keyboard.press("Shift+ArrowRight");
    await page.keyboard.press("PageUp");
    assert.equal((await read(page, "sl-volume")).value, "100");
    // The page asked for a step of 25.
    await page.focus("#sl-big");
    await page.keyboard.press("PageUp");
    assert.equal((await read(page, "sl-big")).value, "125");
    await page.keyboard.press("Shift+ArrowDown");
    assert.equal((await read(page, "sl-big")).value, "100");
    await close();
  });

  test("a slider with decimal steps lands on exact values, and one with named steps says the name", async () => {
    const { page, close } = await open("/flow");
    await page.focus("#sl-tenths");
    await page.keyboard.press("ArrowRight");
    const tenths = await read(page, "sl-tenths");
    assert.equal(tenths.value, "0.4");
    assert.equal(tenths.output, "0.4");
    for (let n = 0; n < 6; n++) await page.keyboard.press("ArrowRight");
    assert.equal((await read(page, "sl-tenths")).value, "1");
    await page.focus("#sl-level");
    assert.equal((await read(page, "sl-level")).text, "Medium");
    await page.keyboard.press("ArrowRight");
    const level = await read(page, "sl-level");
    assert.deepEqual([level.text, level.output, level.value], ["High", "High", "2"]);
    await close();
  });

  test("a slider that takes any value moves by a hundredth of the track on the arrows, in every engine, and by a tenth on Shift and the Page keys", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-mix");
    await page.focus("#sl-mix");
    const at = async () => Number((await read(page, "sl-mix")).value);
    const near = (got, want) => assert.ok(Math.abs(got - want) < 1e-6, `${got} is not ${want}`);
    near(await at(), 0.5);
    await page.keyboard.press("ArrowRight");
    near(await at(), 0.51);
    await page.keyboard.press("ArrowUp");
    near(await at(), 0.52);
    await page.keyboard.press("ArrowLeft");
    near(await at(), 0.51);
    await page.keyboard.press("ArrowDown");
    near(await at(), 0.5);
    await page.keyboard.press("Shift+ArrowRight");
    near(await at(), 0.6);
    await page.keyboard.press("PageDown");
    near(await at(), 0.5);
    await page.keyboard.press("End");
    near(await at(), 1);
    await page.keyboard.press("ArrowRight");
    near(await at(), 1);
    // What is written is the number, to no more than nine places.
    await page.keyboard.press("Home");
    near(await at(), 0);
    await page.keyboard.press("ArrowRight");
    assert.equal((await read(page, "sl-mix")).output, "0.01");
    await close();
  });

  test("a very small step is written as a number, never as an exponent, and the server reads what the browser posts for it and for a drag", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-fine");
    await page.focus("#sl-fine");
    await page.keyboard.press("ArrowRight");
    const now = await read(page, "sl-fine");
    assert.equal(Number(now.value), 1e-7);
    assert.equal(now.output, "0.0000001", "the number, not 1e-7");
    // A drag lands on a double written with many digits.
    await page.evaluate(() => {
      document.getElementById("sl-mix").value = "0.5033333333333333";
    });
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#sl-post")]);
    const posted = await page.locator("#posted").innerText();
    assert.ok(/^fine=(1e-7|0\.0000001)$/m.test(posted), posted);
    assert.ok(/^mix=0\.50333333/m.test(posted), posted);
    const judged = await page.locator("#judged").innerText();
    assert.match(judged, /mix: ok 0\.503333333/);
    assert.match(judged, /fine: ok 0\.0000001/);
    await close();
  });

  test("the value is written with the page's decimal point and posted with a point", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-comma");
    assert.equal((await read(page, "sl-comma")).output, "0,50");
    await page.focus("#sl-comma");
    await page.keyboard.press("ArrowRight");
    const now = await read(page, "sl-comma");
    assert.equal(now.value, "0.75", "the input keeps the point");
    assert.equal(now.output, "0,75");
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#sl-post")]);
    assert.ok((await page.locator("#posted").innerText()).split("\n").includes("menge=0.75"));
    await close();
  });

  test("the two thumbs of a range keep their order and their gap, and the value reads from one to the other", async () => {
    const { page, close } = await open("/flow");
    assert.equal((await read(page, "sl-price-min")).output, "₹200 to ₹800");
    await page.focus("#sl-price-min");
    await page.keyboard.press("End");
    // The gap is two steps of ten.
    assert.equal(await page.locator("#sl-price-min").inputValue(), "780");
    assert.equal(await page.locator("#sl-price-max").inputValue(), "800");
    await page.focus("#sl-price-max");
    await page.keyboard.press("Home");
    assert.equal(await page.locator("#sl-price-max").inputValue(), "800");
    await page.keyboard.press("ArrowRight");
    const both = await read(page, "sl-price-min");
    assert.equal(both.output, "₹780 to ₹810");
    assert.equal(both.fill, "3%", "the filled part is from 78% to 81%");
    assert.equal(both.x, "78%");
    // Each thumb has its own name.
    const names = await page.locator("#sl-price-min, #sl-price-max").evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")));
    assert.deepEqual(names, ["minimum", "maximum"]);
    await close();
  });

  test("a click on the track of a range moves the nearer thumb there", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-price-min");
    const box = await page.locator("#sl-price-min").locator("xpath=ancestor::*[@data-slot='slider-control']").boundingBox();
    const at = (p) => box.x + 7 + (box.width - 14) * p;
    await page.mouse.click(at(0.9), box.y + 12);
    const high = Number(await page.locator("#sl-price-max").inputValue());
    assert.ok(high >= 880 && high <= 920, `the upper thumb went to ${high}`);
    assert.equal(await page.locator("#sl-price-min").inputValue(), "200");
    await page.mouse.click(at(0.1), box.y + 12);
    const low = Number(await page.locator("#sl-price-min").inputValue());
    assert.ok(low >= 80 && low <= 120, `the lower thumb went to ${low}`);
    assert.equal(await page.evaluate(() => document.activeElement.id), "sl-price-min");
    await close();
  });

  test("dragging a thumb with the pointer moves a slider", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-volume");
    const box = await page.locator("#sl-volume").boundingBox();
    const start = box.x + 7 + (box.width - 14) * 0.4;
    await page.mouse.move(start, box.y + 12);
    await page.mouse.down();
    await page.mouse.move(box.x + 7 + (box.width - 14) * 0.7, box.y + 12, { steps: 6 });
    await page.mouse.up();
    const value = Number((await read(page, "sl-volume")).value);
    assert.ok(value >= 65 && value <= 75, `dragged to ${value}`);
    assert.equal((await read(page, "sl-volume")).output, `${value}%`);
    await close();
  });

  test("right to left: the least is at the right, and the arrows move the thumb the way it looks", async () => {
    const { page, close } = await open("/flow");
    await room(page, "sl-rtl-one");
    const thumbAt = async () => {
      // The page's reduced-motion rule leaves a transition of one frame on everything: let it end.
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
      const box = await page.locator("#sl-rtl-one").boundingBox();
      const fill = await page.locator("#sl-rtl-one").evaluate((e) => {
        const b = e.closest("[data-ui-slider]").querySelector("[data-slot=slider-fill]").getBoundingClientRect();
        return { left: b.left, right: b.right, width: b.width };
      });
      return { box, fill };
    };
    await page.focus("#sl-rtl-one");
    await page.keyboard.press("Home");
    assert.equal((await read(page, "sl-rtl-one")).value, "0");
    let { box, fill } = await thumbAt();
    assert.ok(fill.width < 2, "nothing is filled at the least: " + JSON.stringify(fill));
    await page.keyboard.press("ArrowLeft");
    assert.equal((await read(page, "sl-rtl-one")).value, "10", "Left goes toward the left, where there is more");
    await page.keyboard.press("ArrowRight");
    assert.equal((await read(page, "sl-rtl-one")).value, "0");
    await page.keyboard.press("End");
    ({ box, fill } = await thumbAt());
    assert.ok(Math.abs(fill.left - box.x) < 12, "the fill reaches the left end at the most: " + JSON.stringify({ fill, box }));
    assert.ok(Math.abs(fill.right - (box.x + box.width)) < 12, "and starts at the right");
    // A click at the left end is the most.
    await page.mouse.click(box.x + 12, box.y + 12);
    assert.equal((await read(page, "sl-rtl-one")).value, "100");
    // The marks run the same way: the most at the left.
    const at = await page.locator("#sl-rtl-range").evaluate((e) => {
      const shown = [...e.closest("[data-ui-slider]").querySelectorAll("[data-slot=slider-marks] text")].filter((t) => t.getClientRects().length);
      return Object.fromEntries(shown.map((t) => [t.textContent, Math.round(t.getBoundingClientRect().left)]));
    });
    assert.ok(at["100"] < at["0"], `100 is left of 0: ${JSON.stringify(at)}`);
    // The range keeps its order the same way.
    const range = await page.locator("#sl-rtl-range").evaluate((e) => e.value);
    assert.equal(range, "20");
    await close();
  });

  test("a form reset puts the value, the text and the filled part back", async () => {
    const { page, close } = await open("/flow");
    await page.focus("#sl-volume");
    await page.keyboard.press("End");
    await page.focus("#sl-price-min");
    await page.keyboard.press("End");
    await page.evaluate(() => document.getElementById("sl-form").reset());
    await page.waitForFunction(() => document.querySelector("#sl-volume").closest("[data-ui-slider]").querySelector("output").textContent === "40%");
    assert.deepEqual(await read(page, "sl-volume"), { value: "40", text: "40%", output: "40%", fill: "40%", x: "0%" });
    assert.equal((await read(page, "sl-price-min")).output, "₹200 to ₹800");
    await close();
  });

  test("the form posts every slider by its name, and the server holds each value to its grid", async () => {
    const { page, close } = await open("/flow");
    await page.focus("#sl-volume");
    await page.keyboard.press("ArrowRight");
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#sl-post")]);
    const posted = await page.locator("#posted").innerText();
    for (const line of ["volume=45", "tenths=0.3", "price_min=200", "price_max=800", "level=1", "still_min=10", "still_max=60", "big=100", "bad=90"]) {
      assert.ok(posted.split("\n").includes(line), `${line} was posted: ${posted}`);
    }
    assert.ok(!/^off=/m.test(posted), "a disabled slider posts nothing");
    const judged = await page.locator("#judged").innerText();
    assert.match(judged, /volume: ok 45/);
    assert.match(judged, /tenths: ok 0.3/);
    assert.match(judged, /price: ok 200-800/);
    assert.match(judged, /level: ok 1/);
    await close();
  });

  test("the server refuses a value the browser would never send", async () => {
    const { page, close } = await open("/flow");
    const post = async (body) => {
      const response = await page.request.post(BASE + "/flow/echo", { data: body, headers: { "content-type": "application/x-www-form-urlencoded" } });
      assert.equal(response.status(), 200, body);
      const html = await response.text();
      return html.match(/<pre id="judged">([\s\S]*?)<\/pre>/)[1].replaceAll("&#39;", "'");
    };
    const good = "tenths=0.3&price_min=200&price_max=800&level=1&volume=";
    assert.match(await post(good + "37"), /volume: Choose a value in steps of 5\./);
    assert.match(await post(good + "105"), /volume: Choose 100 or less\./);
    assert.match(await post(good + "-5"), /volume: Choose 0 or more\./);
    for (const bad of ["0x10", "NaN", "Infinity", "5%20", "%205", "5,0", "", "9".repeat(400), "%00", "%E2%88%9E", "1e", "e2", "1e999999999"]) {
      assert.match(await post(good + bad), /volume: (Enter a number|Choose a value)\./, bad);
    }
    // What a browser may write for a number is read: an exponent, and a long fraction rounded to nine places.
    assert.match(await post(good + "1e2"), /volume: ok 100/);
    assert.match(await post(good + "1.0e%2B2"), /volume: ok 100/);
    assert.match(await post("volume=5&tenths=0.30000000000000004&price_min=200&price_max=800&level=1"), /tenths: ok 0\.3/);
    assert.match(await post("volume=5&mix=0.5033333333333333&fine=1e-7&tenths=0.3&price_min=200&price_max=800&level=1"), /mix: ok 0\.503333333[\s\S]*fine: ok 0\.0000001/);
    assert.match(await post("volume=5&volume=10&" + good.slice(0, -8)), /volume: Choose one value\./);
    assert.match(await post("volume=5&tenths=0.35&price_min=200&price_max=800&level=1"), /tenths: Choose a value in steps of 0\.1\./);
    assert.match(await post("volume=5&tenths=0.3&price_min=900&price_max=100&level=1"), /price: The lower value can't be above the upper one\./);
    assert.match(await post("volume=5&tenths=0.3&price_min=200&price_max=210&level=1"), /price: The two values must be at least 20 apart\./);
    assert.match(await post("volume=5&tenths=0.3&price_min=200&level=1"), /price: Choose a value\./);
    assert.match(await post("volume=5&tenths=0.3&price_min=200&price_max=800&level=3"), /level: Choose 2 or less\./);
    await close();
  });

  test("with JavaScript off a slider still moves by the keys and posts, and the server judges the post", async () => {
    const { page, close } = await open("/flow", { js: false });
    assert.equal(await page.locator("[data-ui-js]").count(), 0, "no script ran");
    await page.focus("#sl-volume");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    assert.equal(await page.locator("#sl-volume").inputValue(), "50");
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#sl-post")]);
    assert.match(await page.locator("#judged").innerText(), /volume: ok 50/);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in the sliders, and the page does not scroll sideways (${label})`, async () => {
      const { page, close } = await open("/flow", options);
      // The pointer is off the buttons and the colours have settled, so axe reads what a person sees.
      await page.mouse.move(2, 2);
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 80)))));
      assert.deepEqual(await axe(page, "#sliders"), []);
      const width = options.viewport?.width || 1440;
      const past = await page.locator("#sliders").evaluate(
        (e, w) => [...e.querySelectorAll("*:not(g)")].filter((el) => el.getBoundingClientRect().right > w + 1).map((el) => `${el.tagName}.${el.getAttribute("data-slot") || el.id}:${Math.round(el.getBoundingClientRect().right)}`).slice(0, 5),
        width,
      );
      assert.deepEqual(past, [], "nothing past the right edge");
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
      await shot(page, `${browserName}-sliders-${label.replaceAll(" ", "-")}`, "#sliders");
      await shot(page, `${browserName}-steppers-${label.replaceAll(" ", "-")}`, "#steppers");
      await close();
    });
  }

  test("a slider in the page after a form post and after the page comes back from history keeps what the browser restored", async () => {
    const { page, close } = await open("/flow");
    await page.focus("#sl-volume");
    await page.keyboard.press("End");
    await Promise.all([page.waitForURL("**/flow/echo"), page.click("#sl-post")]);
    await page.goBack();
    await page.waitForSelector("#sl-volume");
    const now = await read(page, "sl-volume");
    // Whatever the browser restored, what is written and what is filled agree with the input.
    assert.equal(now.output, `${now.value}%`);
    assert.equal(now.fill, `${now.value}%`);
    await close();
  });
});
