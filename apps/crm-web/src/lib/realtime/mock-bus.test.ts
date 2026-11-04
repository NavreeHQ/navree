import { afterEach, describe, expect, it, vi } from "vitest";
import { connectMockBus } from "./mock-bus";

/** What the mock API's handlers dispatch (`broadcast` in `src/mocks/handlers.ts`). */
function broadcast(channel: string, event: string, data: unknown) {
  window.dispatchEvent(new CustomEvent("navree-mock:broadcast", { detail: { channel, event, data } }));
}

describe("the mock API's real-time connection", () => {
  const connections: ReturnType<typeof connectMockBus>[] = [];
  const connect = () => {
    const connection = connectMockBus();
    connections.push(connection);
    return connection;
  };
  afterEach(() => {
    for (const connection of connections.splice(0)) connection.disconnect();
  });

  it("delivers an event on a private channel to its listeners, and nothing else", () => {
    const deals = vi.fn();
    const connection = connect();
    connection.listen("crm.deals", "deal.updated", deals);

    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    broadcast("private-crm.deals", "deal.deleted", { id: "d1" });
    broadcast("private-users.u1", "deal.updated", { deal: { id: "d2" } });

    expect(deals).toHaveBeenCalledTimes(1);
    expect(deals).toHaveBeenCalledWith({ deal: { id: "d1" } });
  });

  it("stops a listener with the function listen returns", () => {
    const deals = vi.fn();
    const stop = connect().listen("crm.deals", "deal.updated", deals);

    stop();
    broadcast("private-crm.deals", "deal.updated", {});

    expect(deals).not.toHaveBeenCalled();
  });

  it("leaves a channel and closes, which stops every listener and tells the state watchers", () => {
    const deals = vi.fn();
    const notifications = vi.fn();
    const states: string[] = [];
    const connection = connect();
    connection.onStateChange((state) => states.push(state));
    connection.listen("crm.deals", "deal.updated", deals);
    connection.listen("users.u1", "notification.created", notifications);

    connection.leave("crm.deals");
    broadcast("private-crm.deals", "deal.updated", {});
    broadcast("private-users.u1", "notification.created", { title: "New task" });
    expect(deals).not.toHaveBeenCalled();
    expect(notifications).toHaveBeenCalledTimes(1);

    connection.disconnect();
    broadcast("private-users.u1", "notification.created", { title: "Another" });
    expect(notifications).toHaveBeenCalledTimes(1);
    expect(connection.state).toBe("disconnected");
    expect(states).toEqual(["disconnected"]);
  });
});
