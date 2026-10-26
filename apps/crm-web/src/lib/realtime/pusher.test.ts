import type Pusher from "pusher-js";
import type { ChannelAuthorizationCallback, Options } from "pusher-js";
import { describe, expect, it, vi } from "vitest";
import { CHANNEL_AUTH_PATH, connectPusher, pusherOptions, type AuthorizeRequest } from "./pusher";

type Handler = (data: unknown) => void;
type AuthorizationData = Parameters<ChannelAuthorizationCallback>[1];

/** A stand-in for pusher-js: records subscriptions and lets the test emit events. */
class FakeChannel {
  handlers = new Map<string, Handler[]>();
  bind(event: string, handler: Handler) {
    this.handlers.set(event, [...(this.handlers.get(event) ?? []), handler]);
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
  unsubscribed: string[] = [];
  disconnected = false;
  stateHandlers: ((change: { previous: string; current: string }) => void)[] = [];
  connection = {
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
    for (const handler of this.stateHandlers) handler({ previous: "connecting", current });
  }
}

const SERVER = { key: "navree-crm-key", wsHost: "127.0.0.1", wsPort: 8080 };

function connect(authorize: AuthorizeRequest = vi.fn(), onUnavailable?: () => void) {
  const connection = connectPusher({
    server: SERVER,
    authorize,
    onUnavailable,
    PusherClass: FakePusher as unknown as typeof Pusher,
  });
  return { connection, pusher: FakePusher.last };
}

/** Runs pusher-js's channel authorization the way the library does and returns what it got. */
function authorizeChannel(options: Options, socketId: string, channelName: string) {
  const auth = options.channelAuthorization as { customHandler: (...args: unknown[]) => void };
  return new Promise<{ error: Error | null; data: AuthorizationData }>((resolve) =>
    auth.customHandler({ socketId, channelName }, (error: Error | null, data: AuthorizationData) =>
      resolve({ error, data }),
    ),
  );
}

describe("pusher-js connection to Navree's real-time server", () => {
  it("connects with the app key straight to the API's WebSocket, without Pusher's cloud or stats", () => {
    const { pusher } = connect();

    expect(pusher.key).toBe("navree-crm-key");
    expect(pusher.options).toMatchObject({
      wsHost: "127.0.0.1",
      wsPort: 8080,
      wssPort: 8080,
      forceTLS: false,
      enabledTransports: ["ws", "wss"],
      disableStats: true,
    });
  });

  it("authorizes private channels through the BFF with the socket id and channel name", async () => {
    const authorize = vi.fn<AuthorizeRequest>(
      async () => new Response(JSON.stringify({ auth: "navree-crm-key:signature" }), { status: 200 }),
    );
    const options = pusherOptions(SERVER, authorize);

    const result = await authorizeChannel(options, "123.456", "private-crm.deals");

    expect(result).toEqual({ error: null, data: { auth: "navree-crm-key:signature" } });
    expect(authorize).toHaveBeenCalledOnce();
    const [path, init] = authorize.mock.calls[0];
    expect(path).toBe(CHANNEL_AUTH_PATH);
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({ socket_id: "123.456", channel_name: "private-crm.deals" });
  });

  it("reports a refused authorization to pusher-js as an error", async () => {
    const authorize = vi.fn<AuthorizeRequest>(async () => new Response("{}", { status: 403 }));

    const result = await authorizeChannel(pusherOptions(SERVER, authorize), "1.2", "private-users.u2");

    expect(result.data).toBeNull();
    expect(result.error?.message).toBe("Channel authorization failed (403)");
  });

  it("listens on the private channel and hands event payloads to the callback", () => {
    const { connection, pusher } = connect();
    const deals = vi.fn();
    const notifications = vi.fn();

    connection.listen("crm.deals", "deal.updated", deals);
    connection.listen("users.u1", "notification.created", notifications);
    pusher.channels.get("private-crm.deals")!.emit("deal.updated", { deal: { id: "d1" } });

    expect([...pusher.channels.keys()]).toEqual(["private-crm.deals", "private-users.u1"]);
    expect(deals).toHaveBeenCalledWith({ deal: { id: "d1" } });
    expect(notifications).not.toHaveBeenCalled();
  });

  it("subscribes once per channel however many events are bound", () => {
    const { connection, pusher } = connect();
    const subscribe = vi.spyOn(pusher, "subscribe");

    connection.listen("crm.deals", "deal.updated", vi.fn());
    connection.listen("crm.deals", "deal.deleted", vi.fn());

    expect(subscribe).toHaveBeenCalledOnce();
  });

  it("leaves a channel and disconnects", () => {
    const { connection, pusher } = connect();
    const callback = vi.fn();
    connection.listen("crm.deals", "deal.updated", callback);
    const channel = pusher.channels.get("private-crm.deals")!;

    connection.leave("crm.deals");
    channel.emit("deal.updated", {});
    connection.disconnect();

    expect(pusher.unsubscribed).toEqual(["private-crm.deals"]);
    expect(callback).not.toHaveBeenCalled();
    expect(pusher.disconnected).toBe(true);
  });

  it("says once that real time is unavailable, and again only after it reconnected", () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const onUnavailable = vi.fn();
    const { pusher } = connect(vi.fn(), onUnavailable);

    pusher.setState("unavailable");
    pusher.setState("failed");
    expect(onUnavailable).toHaveBeenCalledOnce();

    pusher.setState("connected");
    pusher.setState("unavailable");
    expect(onUnavailable).toHaveBeenCalledTimes(2);
    expect(info).toHaveBeenCalledTimes(2);
  });
});
