import Pusher, { type Channel, type ChannelAuthorizationHandler, type Options } from "pusher-js";
import { api } from "@/lib/api/client";
import { env } from "@/lib/env";
import type { RealtimeConnection } from "./connection";

/** Where Navree's Pusher-protocol server listens, and the app key it expects. */
export interface PusherServer {
  key: string;
  wsHost: string;
  wsPort: number;
  /** wss:// instead of ws:// (a TLS-terminating proxy in front of the API). Default: false. */
  forceTLS?: boolean;
}

/** Sends the channel-authorization request; the app's API client in production, a stub in tests. */
export type AuthorizeRequest = (path: string, init: RequestInit) => Promise<Response>;

/** The BFF route that signs private-channel subscriptions (it adds the bearer token from the cookie). */
export const CHANNEL_AUTH_PATH = "/broadcasting/auth";

/** Private channels carry the protocol's `private-` prefix; callers name channels without it. */
export const privateChannel = (name: string) => `private-${name}`;

/**
 * Authorizes a private-channel subscription through the same-origin BFF
 * (`POST /broadcasting/auth` with `{socket_id, channel_name}`), so the browser
 * never holds a token. The answer is `{auth}` signed by Navree.
 */
export function bffChannelAuthorizer(authorize: AuthorizeRequest): ChannelAuthorizationHandler {
  return ({ socketId, channelName }, callback) => {
    authorize(CHANNEL_AUTH_PATH, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ socket_id: socketId, channel_name: channelName }),
    })
      .then(async (response) => {
        if (!response.ok) throw new Error(`Channel authorization failed (${response.status})`);
        callback(null, await response.json());
      })
      .catch((error: unknown) => callback(error instanceof Error ? error : new Error(String(error)), null));
  };
}

/** pusher-js options for a self-hosted Pusher-protocol server (no cluster, no stats, WebSockets only). */
export function pusherOptions(server: PusherServer, authorize: AuthorizeRequest): Options {
  return {
    // pusher-js requires a cluster; wsHost overrides it, so the value is never used.
    cluster: "mt1",
    wsHost: server.wsHost,
    wsPort: server.wsPort,
    wssPort: server.wsPort,
    forceTLS: server.forceTLS ?? false,
    enabledTransports: ["ws", "wss"],
    disableStats: true,
    channelAuthorization: { customHandler: bffChannelAuthorizer(authorize) },
  };
}

export interface ConnectPusherOptions {
  server?: PusherServer;
  authorize?: AuthorizeRequest;
  /** Called once each time the socket gives up (`unavailable` or `failed`); the app keeps working without it. */
  onUnavailable?: () => void;
  /** The pusher-js class (a fake in tests). */
  PusherClass?: typeof Pusher;
}

/**
 * Connects to the API's Pusher-protocol server with pusher-js. The socket goes
 * straight to Navree; private channels authorize through the BFF.
 */
export function connectPusher({
  server = { key: env.pusherKey, wsHost: env.wsHost, wsPort: env.wsPort },
  authorize = (path, init) => api.rawFetch(path, init),
  onUnavailable,
  PusherClass = Pusher,
}: ConnectPusherOptions = {}): RealtimeConnection {
  const pusher = new PusherClass(server.key, pusherOptions(server, authorize));
  const channels = new Map<string, Channel>();

  // Real-time is optional: if the socket can't connect, queries still refetch
  // on focus and the unread count polls, so just note it once.
  let reported = false;
  pusher.connection.bind("state_change", ({ current }: { previous: string; current: string }) => {
    if ((current === "unavailable" || current === "failed") && !reported) {
      reported = true;
      console.info(`[navree] Real-time updates unavailable (${current}); continuing without them.`);
      onUnavailable?.();
    }
    if (current === "connected") reported = false;
  });

  const subscribe = (name: string): Channel => {
    const full = privateChannel(name);
    let channel = channels.get(full);
    if (!channel) {
      channel = pusher.subscribe(full);
      channels.set(full, channel);
    }
    return channel;
  };

  return {
    listen(channel, event, callback) {
      subscribe(channel).bind(event, callback);
    },
    leave(channel) {
      const full = privateChannel(channel);
      channels.get(full)?.unbind_all();
      channels.delete(full);
      pusher.unsubscribe(full);
    },
    disconnect() {
      for (const channel of channels.values()) channel.unbind_all();
      channels.clear();
      pusher.disconnect();
    },
  };
}
