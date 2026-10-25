import Echo from "laravel-echo";
import Pusher from "pusher-js";
import { api } from "@/lib/api/client";
import { env } from "@/lib/env";
import type { RealtimeConnection } from "./connection";

type AuthCallback = (error: Error | null, data: { auth: string; channel_data?: string } | null) => void;

/**
 * Connects to the API's Pusher-protocol server with Laravel Echo. The socket
 * goes straight to Keel; private channels authorise through same-origin
 * `POST /broadcasting/auth`, where the BFF adds the bearer token.
 */
export function connectEcho(onUnavailable?: () => void): RealtimeConnection {
  const echo = new Echo({
    broadcaster: "pusher",
    Pusher,
    key: env.pusherKey,
    cluster: "mt1",
    wsHost: env.wsHost,
    wsPort: env.wsPort,
    wssPort: env.wsPort,
    forceTLS: false,
    enabledTransports: ["ws", "wss"],
    disableStats: true,
    authorizer: (channel: { name: string }) => ({
      authorize: (socketId: string, callback: AuthCallback) => {
        api
          .rawFetch("/broadcasting/auth", {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "application/json" },
            body: JSON.stringify({ socket_id: socketId, channel_name: channel.name }),
          })
          .then(async (response) => {
            if (!response.ok) throw new Error(`Channel authorization failed (${response.status})`);
            callback(null, await response.json());
          })
          .catch((error: unknown) => callback(error instanceof Error ? error : new Error(String(error)), null));
      },
    }),
  });

  // Real-time is optional: if the socket can't connect the app keeps working
  // with polling/refetch-on-focus, so just note it once.
  const pusher = (echo.connector as unknown as { pusher: Pusher }).pusher;
  let reported = false;
  pusher.connection.bind("state_change", ({ current }: { current: string }) => {
    if ((current === "unavailable" || current === "failed") && !reported) {
      reported = true;
      console.info(`[keel] Real-time updates unavailable (${current}); continuing without them.`);
      onUnavailable?.();
    }
    if (current === "connected") reported = false;
  });

  return {
    listen(channel, event, callback) {
      // Leading dot: the event name is used as-is, without Echo's namespace.
      echo.private(channel).listen(`.${event}`, callback);
    },
    leave(channel) {
      echo.leave(channel);
    },
    disconnect() {
      echo.disconnect();
    },
  };
}
