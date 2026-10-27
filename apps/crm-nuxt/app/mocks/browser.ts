import { setupWorker } from "msw/browser";
import { resetDatabase } from "./db";
import { handlers } from "./handlers";

export const worker = setupWorker(...handlers);

let started: Promise<void> | null = null;

/**
 * Starts the mock API in the browser (`NUXT_PUBLIC_API_MOCKS=1`, from the
 * `01.mocks.client` plugin). Idempotent: a hot reload may run the plugin again.
 */
export function startMockApi(): Promise<void> {
  started ??= worker
    .start({
      // Only API traffic is mocked; Nuxt's assets, HMR and fonts pass through.
      onUnhandledRequest: "bypass",
      quiet: true,
      serviceWorker: { url: "/mockServiceWorker.js" },
    })
    .then(() => {
      Object.assign(window, {
        __navreeMock: {
          reset() {
            resetDatabase();
            window.location.reload();
          },
        },
      });
      console.info(
        "[navree] Mock API enabled. Sign in as rep1@navree.test / password123. Reset with __navreeMock.reset().",
      );
    });
  return started;
}
