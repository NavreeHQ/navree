// An island script of the plugin `proof` for an island with no body: it notes that it was imported.
/** @type {any} */ (globalThis).bareLoaded = true;

export function mount() {
  /** @type {any} */ (globalThis).bareMounted = true;
}
