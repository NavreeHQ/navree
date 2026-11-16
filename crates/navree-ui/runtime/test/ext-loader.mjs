// Loads a UI extension's script, or an island's, in the runtime's tests: the address the extensions module
// imports, `https://app.test/_navree/ui/ext/NAME.js`, is the file `fixtures/ext/NAME.mjs` beside this one, and
// the address the islands module imports, `https://app.test/_navree/ui/island/NAME.js`, is
// `fixtures/island/NAME.mjs`. A name with no file fails the import, as an address the server doesn't serve does.
// Every other specifier resolves as it would.
//
//   import { register } from "node:module";
//   register("./ext-loader.mjs", import.meta.url);

const SERVED = /^https:\/\/app\.test\/_navree\/ui\/(ext|island)\/([a-z0-9_.-]+)\.js$/;

/**
 * @param {string} specifier @param {any} context
 * @param {(specifier: string, context: any) => Promise<{url: string}>} next
 */
export async function resolve(specifier, context, next) {
  const served = SERVED.exec(specifier);
  if (!served) return next(specifier, context);
  return { url: new URL(`./fixtures/${served[1]}/${served[2]}.mjs`, import.meta.url).href, shortCircuit: true };
}
