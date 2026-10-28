/**
 * Replaces Next's `polyfill-module` in the browser bundle (see `turbopack.resolveAlias`
 * in next.config.ts). It patches `Array.prototype.at`, `Object.hasOwn`,
 * `Array.prototype.flat` and a few more methods that every browser Next
 * targets (Chrome and Edge 111, Firefox 111, Safari 16.4) has natively, and
 * `URL.canParse`, which nothing in the site calls.
 */
export {};
