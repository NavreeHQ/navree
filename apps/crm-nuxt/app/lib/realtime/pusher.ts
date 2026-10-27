import Pusher from "pusher-js";
import { api } from "@/lib/api/client";
import type { RealtimeConnection } from "./connection";

type AuthCallback = (error: Error | null, data: { auth: string; channel_data?: string } | null) => void;

export interface PusherSettings {
  key: string;
  wsHost: string;
  wsPort: number;
}

export type ConnectionState = "connecting" | "live" | "unavailable";

/**
 * Connects pusher-js to the API's Pusher-protocol server. The socket goes
 * straight to Navree; private channels authorize through same-origin
 * `POST /broadcasting/auth`, where the BFF adds the bearer token, so the
 * browser never holds a token.
 */
export function connectPusher(settings: PusherSettings, onState?: (state: ConnectionState) => void): RealtimeConnection {
  const pusher = new Pusher(settings.key, {
    cluster: "",
    wsHost: settings.wsHost,
    wsPort: settings.wsPort,
    wssPort: settings.wsPort,
    forceTLS: false,
    enabledTransports: ["ws", "wss"],
    disableStats: true,
    channelAuthorization: {
      customHandler: ({ socketId, channelName }, callback: AuthCallback) => {
        api
          .rawFetch("/broadcasting/auth", {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "application/json" },
            body: JSON.stringify({ socket_id: socketId, channel_name: channelName }),
          })
          .then(async (response) => {
            if (!response.ok) throw new Error(`Channel authorization failed (${response.status})`);
            callback(null, await response.json());
          })
          .catch((error: unknown) => callback(error instanceof Error ? error : new Error(String(error)), null));
      },
    },
  });

  // Real time is optional: when the socket can't connect, the app keeps working
  // with refetch-on-focus and polling, so say so once.
  let reported = false;
  pusher.connection.bind("state_change", ({ current }: { current: string }) => {
    if (current === "unavailable" || current === "failed") {
      if (!reported) console.info(`[navree] Real-time updates unavailable (${current}); continuing without them.`);
      reported = true;
      onState?.("unavailable");
    } else if (current === "connecting") {
      onState?.("connecting");
    } else if (current === "connected") {
      reported = false;
      onState?.("live");
    }
  });

  return {
    listen(channel, event, callback) {
      const name = `private-${channel}`;
      (pusher.channel(name) ?? pusher.subscribe(name)).bind(event, callback);
    },
    leave(channel) {
      pusher.unsubscribe(`private-${channel}`);
    },
    disconnect() {
      pusher.disconnect();
    },
  };
}
