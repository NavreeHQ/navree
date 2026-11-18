// Records every module under `/_navree/` or on an `https:` origin that the runtime asks Node to import in
// `fresh-own-modules.test.mjs`, in `globalThis.imported`, without touching the disk.

/**
 * @param {string} specifier @param {any} context
 * @param {(specifier: string, context: any) => Promise<{url: string}>} next
 */
export async function resolve(specifier, context, next) {
  if (/^(https:\/\/[^/]+)?\/_navree\//.test(specifier) || specifier.startsWith("https://")) {
    return {
      url: `data:text/javascript,${encodeURIComponent(`(globalThis.imported ||= []).push(${JSON.stringify(specifier)});`)}`,
      shortCircuit: true,
    };
  }
  return next(specifier, context);
}
