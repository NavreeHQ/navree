// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/** A stand-in for pusher-js: records its options, channels and bindings. */
const fake = vi.hoisted(() => {
  type Handler = (data: unknown) => void;
  const state = {
    options: undefined as Record<string, unknown> | undefined,
    key: undefined as string | undefined,
    stateHandlers: [] as ((change: { current: string }) => void)[],
    channels: new Map<string, Map<string, Handler[]>>(),
    unsubscribed: [] as string[],
    disconnected: 0,
  };
  class FakePusher {
    connection = {
      bind: (event: string, handler: (change: { current: string }) => void) => {
        if (event === "state_change") state.stateHandlers.push(handler);
      },
    };
    constructor(key: string, options: Record<string, unknown>) {
      state.key = key;
      state.options = options;
    }
    channel(name: string) {
      return state.channels.has(name) ? this.handle(name) : undefined;
    }
    subscribe(name: string) {
      if (!state.channels.has(name)) state.channels.set(name, new Map());
      return this.handle(name);
    }
    unsubscribe(name: string) {
      state.unsubscribed.push(name);
      state.channels.delete(name);
    }
    disconnect() {
      state.disconnected += 1;
    }
    private handle(name: string) {
      const events = state.channels.get(name)!;
      return {
        bind: (event: string, handler: Handler) => {
          events.set(event, [...(events.get(event) ?? []), handler]);
        },
      };
    }
  }
  const emit = (channel: string, event: string, data: unknown) => {
    for (const handler of state.channels.get(channel)?.get(event) ?? []) handler(data);
  };
  const reset = () => {
    state.options = undefined;
    state.key = undefined;
    state.stateHandlers = [];
    state.channels.clear();
    state.unsubscribed = [];
    state.disconnected = 0;
  };
  return { state, FakePusher, emit, reset };
});

vi.mock("pusher-js", () => ({ default: fake.FakePusher }));

const { connectPusher } = await import("@/lib/realtime/pusher");
const { connectMockBus } = await import("@/lib/realtime/mock-bus");
const { DEALS_CHANNEL, userChannel } = await import("@/lib/realtime/events");

type AuthorizerArgs = { socketId: string; channelName: string };
type AuthorizerCallback = (error: Error | null, data: { auth: string } | null) => void;

function authorize(args: AuthorizerArgs): Promise<{ error: Error | null; data: { auth: string } | null }> {
  const handler = (fake.state.options!.channelAuthorization as { customHandler: (a: AuthorizerArgs, cb: AuthorizerCallback) => void })
    .customHandler;
  return new Promise((resolve) => handler(args, (error, data) => resolve({ error, data })));
}

describe("pusher-js connection to Navree's real-time server", () => {
  beforeEach(() => fake.reset());
  afterEach(() => vi.unstubAllGlobals());

  it("connects straight to the API's socket with the app key, over ws only", () => {
    connectPusher({ key: "navree-crm-key", wsHost: "127.0.0.1", wsPort: 8092 });
    expect(fake.state.key).toBe("navree-crm-key");
    expect(fake.state.options).toMatchObject({
      wsHost: "127.0.0.1",
      wsPort: 8092,
      forceTLS: false,
      enabledTransports: ["ws", "wss"],
      disableStats: true,
    });
  });

  it("authorizes private channels through the BFF's same-origin /broadcasting/auth, with no token", async () => {
    const fetch = vi.fn(async (_request: Request) => Response.json({ auth: "navree-crm-key:signature" }));
    vi.stubGlobal("fetch", fetch);
    connectPusher({ key: "k", wsHost: "h", wsPort: 1 });

    const result = await authorize({ socketId: "123.456", channelName: "private-crm.deals" });

    expect(result).toEqual({ error: null, data: { auth: "navree-crm-key:signature" } });
    const request = fetch.mock.calls[0]![0];
    expect(new URL(request.url).pathname).toBe("/broadcasting/auth");
    expect(request.method).toBe("POST");
    expect(request.credentials).toBe("same-origin");
    expect(request.headers.get("authorization")).toBeNull();
    expect(await request.json()).toEqual({ socket_id: "123.456", channel_name: "private-crm.deals" });
  });

  it("hands pusher-js an error when the BFF refuses the channel", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json({ statusCode: 403, message: "Forbidden", error: "Forbidden" }, { status: 403 })),
    );
    connectPusher({ key: "k", wsHost: "h", wsPort: 1 });
    const result = await authorize({ socketId: "1.2", channelName: "private-users.someone-else" });
    expect(result.data).toBeNull();
    expect(result.error?.message).toBe("Channel authorization failed (403)");
  });

  it("reports the socket's state: connecting, live, and unavailable when it can't connect", () => {
    const states: string[] = [];
    vi.spyOn(console, "info").mockImplementation(() => {});
    connectPusher({ key: "k", wsHost: "h", wsPort: 1 }, (state) => states.push(state));
    for (const current of ["connecting", "connected", "unavailable", "failed", "connected"]) {
      for (const handler of fake.state.stateHandlers) handler({ current });
    }
    expect(states).toEqual(["connecting", "live", "unavailable", "unavailable", "live"]);
  });

  it("listens on private channels by their app names, leaves and disconnects", () => {
    const connection = connectPusher({ key: "k", wsHost: "h", wsPort: 1 });
    const updates: unknown[] = [];
    connection.listen(DEALS_CHANNEL, "deal.updated", (data) => updates.push(data));
    connection.listen(DEALS_CHANNEL, "deal.deleted", () => {});

    fake.emit("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    expect(updates).toEqual([{ deal: { id: "d1" } }]);
    expect([...fake.state.channels.keys()]).toEqual(["private-crm.deals"]);

    connection.leave(DEALS_CHANNEL);
    expect(fake.state.unsubscribed).toEqual(["private-crm.deals"]);
    connection.disconnect();
    expect(fake.state.disconnected).toBe(1);
  });
});

describe("the mock bus that stands in for the socket with the mock API", () => {
  const broadcast = (channel: string, event: string, data: unknown) =>
    window.dispatchEvent(new CustomEvent("navree-mock:broadcast", { detail: { channel, event, data } }));

  it("delivers only the events of the channel and event a listener asked for", () => {
    const bus = connectMockBus();
    const seen: unknown[] = [];
    bus.listen(userChannel("u1"), "notification.created", (data) => seen.push(data));

    broadcast("private-users.u1", "notification.created", { title: "Deal assigned" });
    broadcast("private-users.u2", "notification.created", { title: "Not mine" });
    broadcast("private-users.u1", "something.else", {});

    expect(seen).toEqual([{ title: "Deal assigned" }]);
    bus.leave(userChannel("u1"));
    broadcast("private-users.u1", "notification.created", { title: "After leaving" });
    expect(seen).toHaveLength(1);
  });

  it("stops every listener on disconnect", () => {
    const bus = connectMockBus();
    const seen: unknown[] = [];
    bus.listen(DEALS_CHANNEL, "deal.updated", (data) => seen.push(data));
    bus.disconnect();
    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    expect(seen).toEqual([]);
  });
});
