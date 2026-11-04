// The wizard in real browsers, against the proof app's /flow/wizard: each step checked on the
// server before the next, back and the stepper keeping what was typed, a draft that is left and
// resumed, two windows on one draft, a second post of Finish, hostile posts, JavaScript off all the
// way through, and axe in dark, light and on a phone.
//
//   cargo run --release -p ui-proof -- serve          # UI_PROOF_URL, default http://127.0.0.1:8090
//   UI_BROWSERS=chromium,firefox,webkit node --test tests/flow-wizard.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { BASE, axe, hits, inBrowsers, shot } from "./support/flow.mjs";

const PHONE = { width: 390, height: 844 };

inBrowsers(({ open, browserName }) => {
  /** The page's one-frame transitions (reduced motion) have ended: colours are the ones they end on. */
  const quiet = async (page) => {
    // The pointer rests where the last click was: off the button, so its hover colour is not what axe reads.
    await page.mouse.move(2, 2);
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 80)))));
  };
  const idOf = (page) => page.url().split("/")[5];
  const stepOf = (page) => page.url().split("/")[6];
  const heading = (page) => page.locator("[data-slot=wizard-heading]").innerText();
  const states = (page) => page.locator("[data-slot=wizard] [data-slot=step]").evaluateAll((els) => els.map((e) => e.getAttribute("data-state")));
  const go = async (page, action) => {
    await page.locator(`[data-slot=wizard] button[name=_wizard][value="${action}"]:not([hidden])`).first().click();
  };
  /** Presses a button that moves to another step and waits for that step's address. */
  const moveTo = async (page, action, step) => {
    await Promise.all([page.waitForURL(new RegExp(`/flow/wizard/[0-9a-f]+/${step}$`)), go(page, action)]);
    await page.waitForSelector("[data-slot=wizard-heading]");
  };
  const errors = (page) => page.locator("[data-slot=wizard] [data-slot=field-error]").allInnerTexts();
  const start = async (page) => {
    await page.goto(BASE + "/flow/wizard/start");
    await page.waitForSelector("[data-slot=wizard]");
  };
  /** Lets a bad value reach the server: the browser's own checks (`required`, `type=email`) would stop it, as they should, and the server is the authority. */
  const noChecks = (page) => page.evaluate(() => { document.querySelector("[data-slot=wizard-form]").noValidate = true; });
  const first = async (page) => {
    await page.check("[name=kind][value=new]");
    await page.fill("[name=title]", "Annual plan");
    await moveTo(page, "next", "contact");
  };
  const second = async (page) => {
    await page.fill("[name=email]", "ada@example.com");
    await page.fill("[name=code]", "correct horse");
    await moveTo(page, "next", "extras");
  };
  const toReview = async (page) => {
    await first(page);
    await second(page);
    await moveTo(page, "goto:terms", "terms");
    await page.check("[name=accept]");
    await moveTo(page, "next", "review");
  };
  /** The form's fields as the browser would post them, for a post made by hand. */
  const revision = (page) => page.locator("input[name=_rev]").inputValue();

  test("a new wizard opens on its first step, with only that step open", async () => {
    const { page, close } = await open("/flow/wizard/start");
    assert.equal(stepOf(page), "basics");
    assert.equal(await heading(page), "Basics");
    assert.match(await page.title(), /New deal: Basics/);
    assert.deepEqual(await states(page), ["current", "disabled", "disabled", "disabled", "disabled"]);
    assert.equal(await page.locator("[data-slot=wizard] [aria-current=step]").count(), 1);
    assert.equal(await page.locator("button[name=_wizard][value=back]").count(), 0, "no Back on the first step");
    assert.equal(await page.locator("button[name=_wizard][value=next]:not([hidden])").count(), 1);
    assert.equal(await page.locator("[name=_rev]").inputValue(), "0");
    await close();
  });

  test("Next on a step with an error stays, says why, and keeps what was typed", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await page.check("[name=kind][value=existing]");
    // The browser would stop an empty required field: with its checks off, the server says.
    assert.equal(await page.evaluate(() => document.querySelector("[data-slot=wizard-form]").checkValidity()), false, "the browser's own check holds an empty required field");
    await noChecks(page);
    await go(page, "next");
    await page.waitForSelector("[data-slot=field-error]");
    assert.deepEqual(await errors(page), ["Give the deal a name."]);
    assert.equal(stepOf(page), "basics");
    assert.equal(await page.isChecked("[name=kind][value=existing]"), true, "the choice made is still made");
    assert.deepEqual(await states(page), ["current", "disabled", "disabled", "disabled", "disabled"]);
    // Mended, it goes on.
    await page.fill("[name=title]", "Annual plan");
    await moveTo(page, "next", "contact");
    assert.deepEqual(await states(page), ["done", "current", "disabled", "disabled", "disabled"]);
    await close();
  });

  test("Enter in a field presses Next, and not the first button of the stepper", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await page.check("[name=kind][value=new]");
    await page.fill("[name=title]", "Pressed with Enter");
    await Promise.all([page.waitForURL(/\/contact$/), page.press("[name=title]", "Enter")]);
    assert.equal(await heading(page), "Contact");
    await close();
  });

  test("Back and the stepper keep what was typed, valid or not, and show no error for a step not tried", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await first(page);
    await page.fill("[name=email]", "ada@");
    await moveTo(page, "back", "basics");
    assert.equal(await page.inputValue("[name=title]"), "Annual plan");
    // The stepper takes the half typed step back, with the half typed email in it.
    await moveTo(page, "goto:contact", "contact");
    assert.equal(await page.inputValue("[name=email]"), "ada@");
    assert.deepEqual(await errors(page), [], "it was not tried");
    // Tried, it says what is wrong.
    await page.fill("[name=code]", "123456");
    await noChecks(page);
    await go(page, "next");
    await page.waitForSelector("[data-slot=field-error]");
    assert.deepEqual(await errors(page), ["Enter an email address like ada@example.com."]);
    assert.equal(await page.inputValue("[name=email]"), "ada@");
    await close();
  });

  test("a password is typed again: the draft never keeps it, and the step still counts as done", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await first(page);
    await second(page);
    await moveTo(page, "goto:basics", "basics");
    // The stepper shows contact done, and its page has no password in it.
    assert.equal((await states(page))[1], "done");
    await moveTo(page, "goto:contact", "contact");
    assert.equal(await page.inputValue("[name=code]"), "");
    assert.equal(await page.inputValue("[name=email]"), "ada@example.com");
    assert.ok(!(await page.content()).includes("correct horse"));
    await close();
  });

  test("an optional step can be passed over by the stepper, the last step checks its own rule, and the review reads back the answers", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await first(page);
    await second(page);
    assert.equal((await states(page))[3], "upcoming", "the step after an optional one is open");
    await moveTo(page, "goto:terms", "terms");
    await go(page, "next");
    await page.waitForSelector("[data-slot=field-error]");
    assert.deepEqual(await errors(page), ["Accept the terms to finish."]);
    await page.check("[name=accept]");
    await moveTo(page, "next", "review");
    const text = await page.locator("[data-slot=wizard-review]").innerText();
    assert.match(text, /A new customer/);
    assert.match(text, /ada@example\.com/);
    assert.match(text, /Annual plan/);
    assert.doesNotMatch(text, /correct horse/);
    // The contact step has a password that is never kept, and is as done here as in the stepper.
    assert.doesNotMatch(text, /needs attention/);
    assert.equal(await page.locator("[data-slot=wizard-review-step][data-invalid]").count(), 0);
    assert.match(text, /Send me updates\s*No/);
    assert.equal(await page.locator("[data-slot=wizard-review-step] button[value^='goto:']").count(), 4, "an Edit button for each step");
    // An Edit button goes to its step, and the stepper comes back to the review.
    await moveTo(page, "goto:basics", "basics");
    await moveTo(page, "goto:review", "review");
    await close();
  });

  test("Finish makes the deal once, and the draft is gone", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await toReview(page);
    const id = idOf(page);
    await Promise.all([page.waitForURL(/\/done\//), go(page, "finish")]);
    assert.match(await page.locator("[data-slot=alert]").innerText(), /Created: Annual plan/);
    assert.equal((await hits(page))[`made-${id}`], 1);
    // The address of the draft now leads to the deal, and the draft is not there to post to.
    const response = await page.request.post(`${BASE}/flow/wizard/${id}/review`, { data: "_rev=9&_wizard=finish", headers: { "content-type": "application/x-www-form-urlencoded" } });
    assert.equal(response.status(), 404);
    assert.equal((await hits(page))[`made-${id}`], 1);
    await close();
  });

  test("Save and leave keeps the draft, and its address resumes it with what was typed", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await first(page);
    const id = idOf(page);
    await page.fill("[name=email]", "half@typed");
    await Promise.all([page.waitForURL("**/flow?saved=1"), go(page, "save")]);
    await page.waitForSelector("text=Draft saved");
    // From a link, in a new window: the same draft.
    const other = await page.context().newPage();
    await other.goto(`${BASE}/flow/wizard/${id}/contact`);
    assert.equal(await other.inputValue("[name=email]"), "half@typed");
    assert.deepEqual(await states(other), ["done", "current", "disabled", "disabled", "disabled"]);
    // A step that cannot be reached yet sends the person to the one that can.
    await other.goto(`${BASE}/flow/wizard/${id}/terms`);
    assert.equal(stepOf(other), "contact");
    await close();
  });

  test("two windows on one draft: the second to post is told, nothing is lost, and it sees the latest", async () => {
    const { page: a, context, close } = await open("/flow/wizard/start");
    await first(a);
    const id = idOf(a);
    const b = await context.newPage();
    await b.goto(`${BASE}/flow/wizard/${id}/contact`);
    // Window A moves on.
    await a.fill("[name=email]", "ada@example.com");
    await a.fill("[name=code]", "correct horse");
    await moveTo(a, "next", "extras");
    // Window B, drawn before that, posts its own answer.
    await b.fill("[name=email]", "someone@else.example");
    await b.fill("[name=code]", "another code");
    await go(b, "next");
    await b.waitForSelector("[data-slot=alert]");
    assert.match(await b.locator("[data-slot=alert]").innerText(), /changed in another window/);
    assert.equal(await heading(b), "Extras", "it shows the draft as it is now");
    // B's post changed nothing: A's answer is the one kept.
    await b.goto(`${BASE}/flow/wizard/${id}/contact`);
    assert.equal(await b.inputValue("[name=email]"), "ada@example.com");
    await close();
  });

  test("a second post of Finish, even at the same moment, makes one deal", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await toReview(page);
    const id = idOf(page);
    const rev = await revision(page);
    const post = () => page.request.post(`${BASE}/flow/wizard/${id}/review`, { data: `_rev=${rev}&_wizard=finish`, headers: { "content-type": "application/x-www-form-urlencoded" }, maxRedirects: 0 });
    const answers = await Promise.all([post(), post(), post()]);
    const statuses = answers.map((r) => r.status()).sort();
    assert.equal(statuses.filter((s) => s === 302 || s === 303).length, 1, `one Finish got through: ${statuses}`);
    assert.equal((await hits(page))[`made-${id}`], 1);
    await close();
  });

  test("hostile posts are refused and change nothing", async () => {
    const { page, close } = await open("/flow/wizard/start");
    await first(page);
    const id = idOf(page);
    const rev = await revision(page);
    const post = (path, body) => page.request.post(`${BASE}/flow/wizard/${path}`, { data: body, headers: { "content-type": "application/x-www-form-urlencoded" }, maxRedirects: 0 });
    for (const [path, body] of [
      [`${id}/nope`, `_rev=${rev}&_wizard=next`],
      [`${id}/contact`, `_rev=${rev}&_wizard=drop%20table`],
      [`${id}/contact`, `_rev=${rev}&_wizard=goto:..%2f..`],
      [`${id}/terms`, `_rev=${rev}&_wizard=next&accept=true`],
      [`${id}/contact`, `_wizard=next`],
      [`${id}/contact`, `_rev=abc&_wizard=next`],
      [`${id}/contact`, `_rev=${rev}`],
      [`${id}/contact`, `_rev=${rev}&_wizard=next&email=${"x".repeat(100000)}`],
      [`${"a".repeat(8)}/contact`, `_rev=${rev}&_wizard=next`],
      [`..%2F..%2Fetc/contact`, `_rev=${rev}&_wizard=next`],
    ]) {
      const response = await post(path, body);
      assert.ok(response.status() < 500, `${path} ${body.slice(0, 60)} -> ${response.status()}`);
    }
    // The draft is as it was.
    await page.goto(`${BASE}/flow/wizard/${id}/contact`);
    assert.equal(await page.inputValue("[name=email]"), "");
    assert.equal(await revision(page), rev);
    assert.equal((await page.request.get(`${BASE}/flow/wizard/zzzzzzzzzz/basics`)).status(), 404);
    await close();
  });

  test("with JavaScript off every step is a plain page, Enter presses Next, and the whole flow works", async () => {
    const { page, close } = await open("/flow/wizard/start", { js: false });
    assert.equal(await page.locator("[data-ui-js]").count(), 0);
    await page.check("[name=kind][value=new]");
    await page.fill("[name=title]", "No script");
    await Promise.all([page.waitForURL(/\/contact$/), page.press("[name=title]", "Enter")]);
    // A bad answer sent by hand: the page that comes back says what is wrong and keeps the rest.
    const id = idOf(page);
    const rev = await revision(page);
    const sent = await page.request.post(`${BASE}/flow/wizard/${id}/contact`, { data: `_rev=${rev}&_wizard=next&email=bad&code=correct%20horse`, headers: { "content-type": "application/x-www-form-urlencoded" }, maxRedirects: 0 });
    assert.equal(sent.status(), 303);
    await page.goto(`${BASE}/flow/wizard/${id}/contact`);
    assert.deepEqual(await errors(page), ["Enter an email address like ada@example.com."]);
    assert.equal(await page.inputValue("[name=email]"), "bad");
    await page.fill("[name=email]", "ada@example.com");
    await page.fill("[name=code]", "correct horse");
    await Promise.all([page.waitForURL(/\/extras$/), page.click("button[name=_wizard][value=next]:not([hidden])")]);
    await Promise.all([page.waitForURL(/\/terms$/), page.click("[data-slot=step-trigger][value='goto:terms']")]);
    await page.check("[name=accept]");
    await Promise.all([page.waitForURL(/\/review$/), page.click("button[name=_wizard][value=next]:not([hidden])")]);
    await Promise.all([page.waitForURL(/\/done\//), page.click("button[name=_wizard][value=finish]:not([hidden])")]);
    assert.match(await page.locator("[data-slot=alert]").innerText(), /Created: No script/);
    await close();
  });

  test("on a phone the stepper keeps the current step and the count, and the buttons wrap without a sideways scroll", async () => {
    const { page, close } = await open("/flow/wizard/start", { viewport: PHONE });
    await first(page);
    assert.equal((await page.locator("[data-slot=stepper-count]").innerText()).trim(), "Step 2 of 5");
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "no sideways scroll");
    const names = await page.locator("[data-slot=step-text]").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().width > 4));
    assert.deepEqual(names, [false, true, false, false, false]);
    await shot(page, `${browserName}-wizard-390`);
    await close();
  });

  for (const [label, options] of [
    ["dark at 1440", {}],
    ["light at 1440", { light: true }],
    ["dark on a phone", { viewport: PHONE }],
    ["light on a phone", { viewport: PHONE, light: true }],
  ]) {
    test(`axe finds nothing in any step, with errors, or in the review (${label})`, async () => {
      const { page, close } = await open("/flow/wizard/start", options);
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "basics");
      await noChecks(page);
      await go(page, "next");
      await page.waitForSelector("[data-slot=field-error]");
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "basics with an error");
      await shot(page, `${browserName}-wizard-error-${label.replaceAll(" ", "-")}`);
      await first(page);
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "contact");
      await second(page);
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "extras");
      await moveTo(page, "goto:terms", "terms");
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "terms");
      await page.check("[name=accept]");
      await moveTo(page, "next", "review");
      await quiet(page);
      assert.deepEqual(await axe(page, "main"), [], "review");
      await shot(page, `${browserName}-wizard-review-${label.replaceAll(" ", "-")}`);
      await close();
    });
  }
});
