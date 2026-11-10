// A request whose body comes late, on purpose. Two requests whose bodies finish out of order are a race that
// timing makes now and then; here the test makes it: the page's `fetch` gets the headers of the first matching
// answer at once, and the body, read through `text()`, waits until `release()`. The test lets the newer request
// finish first, then releases the older one, and looks at what the older one left behind.

/**
 * Holds the body of the first answer, from now on, to a request whose address matches `url` (and which carries
 * `header`, when one is named).
 * @param {import("playwright").Page} page
 * @param {{url?: RegExp, header?: string}} [match]
 */
export async function lateBody(page, { url, header } = {}) {
  await page.evaluate(([source, name]) => {
    const wanted = source === null ? null : new RegExp(source);
    const real = window.fetch;
    let open;
    const held = new Promise((resolve) => (open = resolve));
    const state = (window.__late = { arrived: false, held, open });
    window.fetch = async function (input, init) {
      const response = await real.call(this, input, init);
      const address = String(input instanceof Request ? input.url : input);
      const headers = (init && init.headers) || {};
      if (!state.arrived && (!wanted || wanted.test(address)) && (!name || name in headers)) {
        state.arrived = true;
        const text = response.text.bind(response);
        response.text = async () => {
          const body = await text();
          await state.held;
          return body;
        };
      }
      return response;
    };
  }, [url ? url.source : null, header ?? null]);
  return {
    /** Resolves once the page has the headers of the held answer. */
    arrived: () => page.waitForFunction(() => window.__late.arrived),
    /** Lets the held body through, and returns once what the page does with it at once has been done. */
    release: () => page.evaluate(async () => {
      window.__late.open();
      await new Promise((done) => setTimeout(done, 0));
    }),
  };
}
