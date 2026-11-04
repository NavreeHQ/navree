// @vitest-environment happy-dom
import { createRealtimeController } from "@navree/nuxt/realtime";
import { describe, expect, it } from "vitest";
import { DEALS_CHANNEL, userChannel } from "@/lib/realtime/events";
import { connectMockBus } from "@/lib/realtime/mock-bus";

// The socket itself is @navree/client's; these tests never open one.

const broadcast = (channel: string, event: string, data: unknown) =>
  window.dispatchEvent(new CustomEvent("navree-mock:broadcast", { detail: { channel, event, data } }));

describe("the mock bus that stands in for the socket with the mock API", () => {
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
    bus.disconnect();
  });

  it("stops a listener with the function listen returns", () => {
    const bus = connectMockBus();
    const seen: unknown[] = [];
    const stop = bus.listen(DEALS_CHANNEL, "deal.updated", (data) => seen.push(data));

    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    stop();
    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d2" } });

    expect(seen).toEqual([{ deal: { id: "d1" } }]);
    bus.disconnect();
  });

  it("stops every listener on disconnect", () => {
    const bus = connectMockBus();
    const seen: unknown[] = [];
    bus.listen(DEALS_CHANNEL, "deal.updated", (data) => seen.push(data));
    bus.disconnect();
    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    expect(seen).toEqual([]);
    expect(bus.state).toBe("disconnected");
  });

  it("is the connection the module's controller hands to useChannel", async () => {
    const controller = createRealtimeController();
    expect(controller.state.value).toBe("disconnected");

    await controller.connect(() => connectMockBus());

    expect(controller.state.value).toBe("connected");
    const seen: unknown[] = [];
    controller.connection.value!.listen(DEALS_CHANNEL, "deal.updated", (data) => seen.push(data));
    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d1" } });
    expect(seen).toEqual([{ deal: { id: "d1" } }]);

    controller.disconnect();
    expect(controller.state.value).toBe("disconnected");
    broadcast("private-crm.deals", "deal.updated", { deal: { id: "d2" } });
    expect(seen).toHaveLength(1);
  });
});
