import type { ChannelAuthorizationCallback, Options } from "pusher-js";
import { describe, expect, it, vi } from "vitest";
import {
  channelAuthorizer,
  channelName,
  connectRealtime,
  createMemoryRealtime,
  publicChannel,
  realtimeOptions,
  type AuthorizeRequest,
  type PusherClass,
} from "./realtime.js";

type Handler = (data: unknown) => void;
type AuthorizationData = Parameters<ChannelAuthorizationCallback>[1];

/** A stand-in for pusher-js: records subscriptions and lets a test emit events. */
class FakeChannel {
  handlers = new Map<string, Handler[]>();
  bind(event: string, handler: Handler) {
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), handler]);
    return this;
  }
  unbind(event: string, handler: Handler) {
    this.handlers.set(
      event,
      (this.handlers.get(event) ?? []).filter((h) => h !== handler),
    );
    return this;
  }
  unbind_all() {
    this.handlers.clear();
    return this;
  }
  emit(event: string, data: unknown) {
    for (const handler of this.handlers.get(event) ?? []) handler(data);
  }
}

class FakePusher {
  static last: FakePusher;
  channels = new Map<string, FakeChannel>();
  subscribed: string[] = [];
  unsubscribed: string[] = [];
  disconnected = false;
  stateHandlers: ((change: { previous: string; current: string }) => void)[] = [];
  connection = {
    state: "connecting",
    bind: (event: string, handler: (change: { previous: string; current: string }) => void) => {
      if (event === "state_change") this.stateHandlers.push(handler);
    },
  };
  constructor(
    public key: string,
    public options: Options,
  ) {
    FakePusher.last = this;
  }
  subscribe(name: string) {
    this.subscribed.push(name);
    const channel = new FakeChannel();
    this.channels.set(name, channel);
    return channel;
  }
  unsubscribe(name: string) {
    this.unsubscribed.push(name);
  }
  disconnect() {
    this.disconnected = true;
  }
  setState(current: string) {
    this.connection.state = current;
    for (const handler of this.stateHandlers) handler({ previous: "connecting", current });
  }
}

const SERVER = { key: "app-key", wsHost: "127.0.0.1", wsPort: 8080 };

function connect(options: { authorize?: AuthorizeRequest; onUnavailable?: () => void } = {}) {
  const connection = connectRealtime({
    server: SERVER,
    authorize: options.authorize ?? vi.fn(),
    onUnavailable: options.onUnavailable,
    PusherClass: FakePusher as unknown as PusherClass,
  });
  return { connection, pusher: FakePusher.last };
}

function authorizeChannel(options: Options, socketId: string, name: string) {
  const auth = options.channelAuthorization as { customHandler: (...args: unknown[]) => void };
  return new Promise<{ error: Error | null; data: AuthorizationData }>((resolve) =>
    auth.customHandler({ socketId, channelName: name }, (error: Error | null, data: AuthorizationData) =>
      resolve({ error, data }),
    ),
  );
}

describe("channel names", () => {
  it("makes bare names private and keeps explicit prefixes", () => {
    expect(channelName("crm.deals")).toBe("private-crm.deals");
    expect(channelName("private-crm.deals")).toBe("private-crm.deals");
    expect(channelName("presence-deals.42")).toBe("presence-deals.42");
    expect(channelName(publicChannel("status"))).toBe("status");
  });
});

describe("pusher-js options for Navree's real-time server", () => {
  it("points pusher-js at the app's own server, WebSockets only, no stats", () => {
    const options = realtimeOptions({ ...SERVER, forceTLS: true }, vi.fn());
    expect(options).toMatchObject({
      wsHost: "127.0.0.1",
      wsPort: 8080,
      wssPort: 8080,
      forceTLS: true,
      enabledTransports: ["ws", "wss"],
      disableStats: true,
    });
  });

  it("authorizes private channels through POST /broadcasting/auth on the app's own origin", async () => {
    const authorize = vi.fn<AuthorizeRequest>(async () => Response.json({ auth: "app-key:sig" }));
    const options = realtimeOptions(SERVER, authorize);

    const result = await authorizeChannel(options, "1.2", "private-crm.deals");

    expect(result).toEqual({ error: null, data: { auth: "app-key:sig" } });
    const [path, init] = authorize.mock.calls[0]!;
    expect(path).toBe("/broadcasting/auth");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({ socket_id: "1.2", channel_name: "private-crm.deals" });
  });

  it("reports a refused authorization to pusher-js as an error", async () => {
    const handler = channelAuthorizer(async () => new Response(null, { status: 403 }), "/custom/auth");
    const result = await new Promise<{ error: Error | null }>((resolve) =>
      handler({ socketId: "1.2", channelName: "private-x" }, (error) => resolve({ error })),
    );
    expect(result.error?.message).toBe("Channel authorization failed (403)");
  });
});

describe("connectRealtime", () => {
  it("subscribes once per channel and delivers events to every listener", () => {
    const { connection, pusher } = connect();
    const a = vi.fn();
    const b = vi.fn();

    connection.listen("crm.deals", "deal.updated", a);
    connection.listen("crm.deals", "deal.updated", b);
    pusher.channels.get("private-crm.deals")!.emit("deal.updated", { deal: { id: "d1" } });

    expect(pusher.subscribed).toEqual(["private-crm.deals"]);
    expect(a).toHaveBeenCalledWith({ deal: { id: "d1" } });
    expect(b).toHaveBeenCalledTimes(1);
  });

  it("leaves a channel when its last listener stops", () => {
    const { connection, pusher } = connect();
    const stopA = connection.listen("users.u1", "notification.created", vi.fn());
    const stopB = connection.listen("users.u1", "notification.read", vi.fn());

    stopA();
    expect(pusher.unsubscribed).toEqual([]);
    stopB();
    stopB();
    expect(pusher.unsubscribed).toEqual(["private-users.u1"]);

    connection.listen("users.u1", "notification.created", vi.fn());
    expect(pusher.subscribed).toEqual(["private-users.u1", "private-users.u1"]);
  });

  it("leave() and disconnect() drop every listener", () => {
    const { connection, pusher } = connect();
    const callback = vi.fn();
    connection.listen("crm.deals", "deal.updated", callback);
    connection.leave("crm.deals");
    expect(pusher.unsubscribed).toEqual(["private-crm.deals"]);

    connection.listen(publicChannel("status"), "changed", callback);
    connection.disconnect();
    expect(pusher.unsubscribed).toEqual(["private-crm.deals", "status"]);
    expect(pusher.disconnected).toBe(true);
  });

  it("reports the socket's state, and `unavailable` once until it reconnects", () => {
    const onUnavailable = vi.fn();
    const { connection, pusher } = connect({ onUnavailable });
    const states: string[] = [];
    const stop = connection.onStateChange((state) => states.push(state));
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);

    pusher.setState("unavailable");
    pusher.setState("failed");
    pusher.setState("connected");
    stop();
    pusher.setState("unavailable");

    expect(states).toEqual(["unavailable", "failed", "connected"]);
    expect(connection.state).toBe("unavailable");
    expect(onUnavailable).toHaveBeenCalledTimes(2);
    expect(info).toHaveBeenCalledTimes(2);
  });
});

describe("createMemoryRealtime (tests and mock mode)", () => {
  it("delivers emitted events to listeners of the same channel and event", () => {
    const realtime = createMemoryRealtime();
    const callback = vi.fn();
    const stop = realtime.listen("crm.deals", "deal.updated", callback);

    realtime.emit("crm.deals", "deal.updated", { id: 1 });
    realtime.emit("private-crm.deals", "deal.updated", { id: 2 });
    realtime.emit("crm.deals", "deal.deleted", { id: 3 });
    realtime.emit("users.u1", "deal.updated", { id: 4 });

    expect(callback.mock.calls).toEqual([[{ id: 1 }], [{ id: 2 }]]);
    expect(realtime.listening("crm.deals")).toBe(true);
    stop();
    expect(realtime.listening("crm.deals")).toBe(false);
    expect(realtime.state).toBe("connected");
  });

  it("forgets every listener on disconnect", () => {
    const realtime = createMemoryRealtime();
    const callback = vi.fn();
    realtime.listen("a", "e", callback);
    realtime.disconnect();
    realtime.emit("a", "e", 1);
    expect(callback).not.toHaveBeenCalled();
    expect(realtime.state).toBe("disconnected");
  });
});
