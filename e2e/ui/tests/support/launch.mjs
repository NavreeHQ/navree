// How every suite launches its browser. One Firefox process keeps the idle keep-alive connections of every
// context it opens, so over a suite they pile up, hundreds from one address, against the server's limit for an
// address as real users' connections don't. Firefox closes an idle connection after 5 s here instead of 115 s.

const FIREFOX_PREFS = { "network.http.keep-alive.timeout": 5 };

/**
 * Launches `engine` (chromium, firefox or webkit from playwright) with the suite's `options`. Firefox also gets
 * the preferences above, merged with any `firefoxUserPrefs` the suite asks for.
 */
export function launch(engine, options = {}) {
  if (engine.name() !== "firefox") return engine.launch(options);
  return engine.launch({ ...options, firefoxUserPrefs: { ...options.firefoxUserPrefs, ...FIREFOX_PREFS } });
}
