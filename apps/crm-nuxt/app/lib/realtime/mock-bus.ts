import { createMemoryRealtime, type MemoryRealtime } from "@navree/client/realtime";

/** Mirrors `MOCK_BROADCAST_EVENT` in `app/mocks/handlers.ts` (kept separate so mocks never load in production). */
const MOCK_BROADCAST_EVENT = "navree-mock:broadcast";

/**
 * In mock mode, the MSW handlers broadcast on a window event bus instead of a socket. This is the
 * connection the module's `useChannel` listens on in that case: an in-memory real-time connection
 * that plays back what the handlers broadcast.
 */
export function connectMockBus(): MemoryRealtime {
  const memory = createMemoryRealtime();
  const disconnect = memory.disconnect.bind(memory);
  const forward = (event: Event) => {
    const { channel, event: name, data } = (event as CustomEvent<{ channel: string; event: string; data: unknown }>).detail;
    memory.emit(channel, name, data);
  };
  window.addEventListener(MOCK_BROADCAST_EVENT, forward);
  return Object.assign(memory, {
    disconnect() {
      window.removeEventListener(MOCK_BROADCAST_EVENT, forward);
      disconnect();
    },
  });
}
