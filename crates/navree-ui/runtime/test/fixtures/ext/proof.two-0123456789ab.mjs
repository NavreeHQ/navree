// A component written for a contract this runtime doesn't run.
export const contract = 2;

export function mount() {
  /** @type {any} */ (globalThis).mountedTwo = true;
}
