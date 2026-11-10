/**
 * Real time on Navree's Pusher-protocol server with pusher-js. The socket
 * goes straight to the API's WebSocket listener; private and presence
 * channels are authorized with `POST /broadcasting/auth` on the page's own
 * origin, so a backend-for-frontend adds the bearer token and the browser
 * never holds one.
 *
 * `@navree/client/realtime` needs `pusher-js` (an optional peer dependency).
 */
import PusherImport, { type Channel, type ChannelAuthorizationHandler, type Options } from "pusher-js";

/** The pusher-js class. */
export type PusherClass = typeof PusherImport.default;

/**
 * pusher-js is a CommonJS bundle: under Node's ESM rules its default import is the
 * class itself, while bundlers that honour `__esModule` hand out `{ default }`.
 */
const Pusher: PusherClass =
  (PusherImport as unknown as { default?: PusherClass }).default ?? (PusherImport as unknown as PusherClass);

/** Where the API's real-time server listens, and the key it expects (`realtime.key`). */
export interface RealtimeServer {
  key: string;
  wsHost: string;
  wsPort: number;
  /** `wss://` behind a TLS-terminating proxy. Default false. */
  forceTLS?: boolean;
}

/** Sends the channel-authorization request: `client.raw` in an app, a stub in tests. */
export type AuthorizeRequest = (path: string, init: RequestInit) => Promise<Response>;

/** The route that signs private and presence subscriptions. */
export const CHANNEL_AUTH_PATH = "/broadcasting/auth";

/** A public channel (no authorization). */
export interface PublicChannel {
  public: string;
}

/** A channel: a bare name is private (`crm.deals` → `private-crm.deals`). */
export type ChannelRef = string | PublicChannel;

/** Marks `name` as a public channel. */
export function publicChannel(name: string): PublicChannel {
  return { public: name };
}

/** The protocol name of a channel: bare names get `private-`; `private-` and `presence-` names stay. */
export function channelName(channel: ChannelRef): string {
  if (typeof channel !== "string") return channel.public;
  if (channel.startsWith("private-") || channel.startsWith("presence-")) return channel;
  return `private-${channel}`;
}

/** pusher-js's custom channel authorizer, sending `{socket_id, channel_name}` to `path`. */
export function channelAuthorizer(authorize: AuthorizeRequest, path = CHANNEL_AUTH_PATH): ChannelAuthorizationHandler {
  return ({ socketId, channelName: name }, callback) => {
    authorize(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ socket_id: socketId, channel_name: name }),
    })
      .then(async (response) => {
        if (!response.ok) throw new Error(`Channel authorization failed (${response.status})`);
        callback(null, await response.json());
      })
      .catch((error: unknown) => callback(error instanceof Error ? error : new Error(String(error)), null));
  };
}

/** pusher-js options for a self-hosted Pusher-protocol server: no cluster, no stats, WebSockets only. */
export function realtimeOptions(server: RealtimeServer, authorize: AuthorizeRequest, authPath = CHANNEL_AUTH_PATH): Options {
  return {
    // pusher-js requires a cluster; wsHost overrides it, so the value is never used.
    cluster: "mt1",
    wsHost: server.wsHost,
    wsPort: server.wsPort,
    wssPort: server.wsPort,
    forceTLS: server.forceTLS ?? false,
    enabledTransports: ["ws", "wss"],
    disableStats: true,
    channelAuthorization: { customHandler: channelAuthorizer(authorize, authPath) },
  };
}

/** The real-time connection the framework packages use (pusher-js, or the in-memory fake). */
export interface RealtimeConnection {
  /**
   * Calls `callback` with the data of every `event` on `channel`. Returns a
   * function that stops it; a channel is left when its last listener stops.
   */
  listen<T = unknown>(channel: ChannelRef, event: string, callback: (data: T) => void): () => void;
  /** Stops every listener of `channel` and leaves it. */
  leave(channel: ChannelRef): void;
  /** Closes the socket. */
  disconnect(): void;
  /** `initialized`, `connecting`, `connected`, `unavailable`, `failed` or `disconnected`. */
  readonly state: string;
  /** Calls `callback` on every state change; returns a function that stops it. */
  onStateChange(callback: (state: string) => void): () => void;
}

export interface ConnectRealtimeOptions {
  server: RealtimeServer;
  /** Sends `POST /broadcasting/auth`. Default: `fetch` on the page's own origin with cookies. */
  authorize?: AuthorizeRequest;
  /** The authorization route. Default `/broadcasting/auth`. */
  authPath?: string;
  /** Called once each time the socket gives up (`unavailable` or `failed`); the app should keep working. */
  onUnavailable?: () => void;
  /** The pusher-js class (a fake in tests). */
  PusherClass?: PusherClass;
}

type Listener = { event: string; callback: (data: unknown) => void };

const sameOrigin: AuthorizeRequest = (path, init) => globalThis.fetch(path, { credentials: "same-origin", ...init });

/** Connects to the API's real-time server with pusher-js. */
export function connectRealtime({
  server,
  authorize = sameOrigin,
  authPath,
  onUnavailable,
  PusherClass = Pusher,
}: ConnectRealtimeOptions): RealtimeConnection {
  const pusher = new PusherClass(server.key, realtimeOptions(server, authorize, authPath));
  const channels = new Map<string, { channel: Channel; listeners: Set<Listener> }>();
  const stateListeners = new Set<(state: string) => void>();
  let state: string = (pusher.connection as { state?: string }).state ?? "initialized";

  // Real time is optional: if the socket can't connect, queries still refetch, so say so once.
  let reported = false;
  pusher.connection.bind("state_change", ({ current }: { previous: string; current: string }) => {
    state = current;
    if ((current === "unavailable" || current === "failed") && !reported) {
      reported = true;
      console.info(`[navree] Real-time updates unavailable (${current}); continuing without them.`);
      onUnavailable?.();
    }
    if (current === "connected") reported = false;
    for (const listener of stateListeners) listener(current);
  });

  function leaveName(name: string): void {
    const entry = channels.get(name);
    if (!entry) return;
    entry.channel.unbind_all();
    channels.delete(name);
    pusher.unsubscribe(name);
  }

  return {
    listen<T>(channel: ChannelRef, event: string, callback: (data: T) => void) {
      const name = channelName(channel);
      let entry = channels.get(name);
      if (!entry) {
        entry = { channel: pusher.subscribe(name), listeners: new Set() };
        channels.set(name, entry);
      }
      const listener: Listener = { event, callback: callback as (data: unknown) => void };
      entry.listeners.add(listener);
      entry.channel.bind(event, listener.callback);
      const current = entry;
      let stopped = false;
      return () => {
        if (stopped) return;
        stopped = true;
        current.channel.unbind(event, listener.callback);
        current.listeners.delete(listener);
        if (current.listeners.size === 0 && channels.get(name) === current) leaveName(name);
      };
    },
    leave(channel) {
      leaveName(channelName(channel));
    },
    disconnect() {
      for (const name of [...channels.keys()]) leaveName(name);
      stateListeners.clear();
      pusher.disconnect();
    },
    get state() {
      return state;
    },
    onStateChange(callback) {
      stateListeners.add(callback);
      return () => {
        stateListeners.delete(callback);
      };
    },
  };
}

/** The in-memory connection: `emit` plays the server. For tests and mock modes. */
export interface MemoryRealtime extends RealtimeConnection {
  /** Delivers `data` to the listeners of `event` on `channel`. */
  emit(channel: ChannelRef, event: string, data: unknown): void;
  /** True while `channel` has listeners. */
  listening(channel: ChannelRef): boolean;
}

/** A {@link RealtimeConnection} without a socket. */
export function createMemoryRealtime(): MemoryRealtime {
  const channels = new Map<string, Set<Listener>>();
  const stateListeners = new Set<(state: string) => void>();
  let state = "connected";

  const setState = (next: string) => {
    state = next;
    for (const listener of stateListeners) listener(next);
  };

  return {
    listen<T>(channel: ChannelRef, event: string, callback: (data: T) => void) {
      const name = channelName(channel);
      const listeners = channels.get(name) ?? new Set<Listener>();
      channels.set(name, listeners);
      const listener: Listener = { event, callback: callback as (data: unknown) => void };
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) channels.delete(name);
      };
    },
    leave(channel) {
      channels.delete(channelName(channel));
    },
    disconnect() {
      channels.clear();
      setState("disconnected");
    },
    get state() {
      return state;
    },
    onStateChange(callback) {
      stateListeners.add(callback);
      return () => {
        stateListeners.delete(callback);
      };
    },
    emit(channel, event, data) {
      for (const listener of [...(channels.get(channelName(channel)) ?? [])]) {
        if (listener.event === event) listener.callback(data);
      }
    },
    listening(channel) {
      return (channels.get(channelName(channel))?.size ?? 0) > 0;
    },
  };
}
