import { createMemoryRealtime } from "@navree/client/realtime";
import { act, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { RealtimeProvider, useChannel, useRealtimeState } from "./realtime.js";

function DealFeed({ channel = "crm.deals" as string | null }) {
  const [seen, setSeen] = useState<string[]>([]);
  useChannel<{ id: string }>(channel, "deal.updated", (data) => setSeen((s) => [...s, data.id]));
  const state = useRealtimeState();
  return (
    <p>
      {state}: {seen.join(",")}
    </p>
  );
}

describe("RealtimeProvider and useChannel", () => {
  it("opens one connection and delivers a channel's events while mounted", async () => {
    const realtime = createMemoryRealtime();
    const connect = vi.fn(() => realtime);
    const { unmount } = render(
      <RealtimeProvider connect={connect}>
        <DealFeed />
        <DealFeed />
      </RealtimeProvider>,
    );
    await waitFor(() => expect(realtime.listening("crm.deals")).toBe(true));

    act(() => realtime.emit("crm.deals", "deal.updated", { id: "d1" }));

    expect(screen.getAllByText("connected: d1")).toHaveLength(2);
    expect(connect).toHaveBeenCalledTimes(1);
    unmount();
    expect(realtime.listening("crm.deals")).toBe(false);
    expect(realtime.state).toBe("disconnected");
  });

  it("listens to nothing for a null channel, and nothing at all when disabled", async () => {
    const realtime = createMemoryRealtime();
    const connect = vi.fn(() => realtime);
    render(
      <RealtimeProvider connect={connect} enabled={false}>
        <DealFeed />
      </RealtimeProvider>,
    );
    expect(connect).not.toHaveBeenCalled();
    expect(screen.getByText(/disconnected/)).toBeTruthy();

    const other = createMemoryRealtime();
    render(
      <RealtimeProvider connect={() => other}>
        <DealFeed channel={null} />
      </RealtimeProvider>,
    );
    await waitFor(() => expect(screen.getAllByText(/connected/).length).toBeGreaterThan(0));
    expect(other.listening("crm.deals")).toBe(false);
  });

  it("keeps the subscription when the handler changes, and moves it when the channel does", async () => {
    const realtime = createMemoryRealtime();
    const listen = vi.spyOn(realtime, "listen");
    const { rerender } = render(
      <RealtimeProvider connect={() => realtime}>
        <DealFeed channel="users.u1" />
      </RealtimeProvider>,
    );
    await waitFor(() => expect(realtime.listening("users.u1")).toBe(true));
    rerender(
      <RealtimeProvider connect={() => realtime}>
        <DealFeed channel="users.u1" />
      </RealtimeProvider>,
    );
    expect(listen).toHaveBeenCalledTimes(1);

    rerender(
      <RealtimeProvider connect={() => realtime}>
        <DealFeed channel="users.u2" />
      </RealtimeProvider>,
    );
    await waitFor(() => expect(realtime.listening("users.u2")).toBe(true));
    expect(realtime.listening("users.u1")).toBe(false);
  });

  it("keeps the app working when the connection can't be made", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    render(
      <RealtimeProvider
        connect={() => {
          throw new Error("no socket");
        }}
      >
        <DealFeed />
      </RealtimeProvider>,
    );
    await waitFor(() => expect(info).toHaveBeenCalled());
    expect(screen.getByText(/disconnected/)).toBeTruthy();
  });
});
