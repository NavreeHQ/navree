import Pusher, { type ChannelAuthorizationHandler, type Options } from "pusher-js";
import { env } from "@/lib/env";

/** The BFF route that signs private-channel subscriptions (it adds the bearer token from the cookie). */
export const CHANNEL_AUTH_PATH = "/broadcasting/auth";

/** The private channel of a workspace's live updates (the API's `ChannelsModule`). */
export const workspaceChannel = (workspaceId: string) => `private-workspace.${workspaceId}`;

/**
 * Authorizes a private-channel subscription through the same-origin BFF
 * (`POST /broadcasting/auth` with `{socket_id, channel_name}`), so the browser never holds a
 * token. The answer is `{auth}`, signed by the API.
 */
export function bffChannelAuthorizer(send: typeof fetch = fetch): ChannelAuthorizationHandler {
  return ({ socketId, channelName }, callback) => {
    send(CHANNEL_AUTH_PATH, {
      method: "POST",
      headers: { "content-type": "application/json", accept: "application/json" },
      body: JSON.stringify({ socket_id: socketId, channel_name: channelName }),
      credentials: "same-origin",
    })
      .then(async (response) => {
        if (!response.ok) throw new Error(`Channel authorization failed (${response.status})`);
        callback(null, await response.json());
      })
      .catch((error: unknown) => callback(error instanceof Error ? error : new Error(String(error)), null));
  };
}

/** pusher-js options for the API's own Pusher-protocol server (no cluster, no stats, WebSockets only). */
export function pusherOptions(send?: typeof fetch): Options {
  return {
    // pusher-js requires a cluster; wsHost overrides it.
    cluster: "mt1",
    wsHost: env.realtime.host,
    wsPort: env.realtime.port,
    wssPort: env.realtime.port,
    forceTLS: env.realtime.tls,
    enabledTransports: ["ws", "wss"],
    disableStats: true,
    channelAuthorization: { customHandler: bffChannelAuthorizer(send) },
  };
}

/**
 * Listens to `event` on the workspace's channel; returns the function that stops. Real time is
 * optional: when the socket can't connect, screens still refetch on focus.
 */
export function listenToWorkspace(
  workspaceId: string,
  event: string,
  callback: (data: unknown) => void,
): () => void {
  if (!env.realtime.key) return () => undefined;
  const pusher = new Pusher(env.realtime.key, pusherOptions());
  const channel = pusher.subscribe(workspaceChannel(workspaceId));
  channel.bind(event, callback);
  return () => {
    channel.unbind_all();
    pusher.unsubscribe(workspaceChannel(workspaceId));
    pusher.disconnect();
  };
}
