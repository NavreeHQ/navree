import { createMemoryRealtime } from "@navree/client/realtime";
import { RealtimeProvider } from "@navree/next/realtime";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { toast } from "sonner";
import { afterEach, describe, expect, it, vi } from "vitest";
import { keys } from "@/lib/api/keys";
import { RealtimeBridge } from "./realtime-bridge";

vi.mock("sonner", () => ({ toast: vi.fn() }));
// The bell pulls in the whole notification UI; the bridge only needs how a notification reads.
vi.mock("@/components/app-shell/notifications-bell", () => ({
  notificationParts: (n: { type: string; data: Record<string, unknown> }) => ({
    title: String(n.data.title ?? n.type),
    message: typeof n.data.message === "string" ? n.data.message : undefined,
  }),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function mountBridge() {
  const realtime = createMemoryRealtime();
  const queryClient = new QueryClient();
  const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue();
  render(
    <QueryClientProvider client={queryClient}>
      <RealtimeProvider connect={() => realtime}>
        <RealtimeBridge userId="u7" />
      </RealtimeProvider>
    </QueryClientProvider>,
  );
  await waitFor(() => expect(realtime.listening("crm.deals")).toBe(true));
  return { realtime, invalidate };
}

describe("RealtimeBridge", () => {
  it("listens on the deals channel and the signed-in user's channel", async () => {
    const { realtime } = await mountBridge();

    expect(realtime.listening("crm.deals")).toBe(true);
    expect(realtime.listening("users.u7")).toBe(true);
    expect(realtime.listening("users.u8")).toBe(false);
  });

  it("refreshes the deal, the board, the lists and the dashboard when a deal changes", async () => {
    const { realtime, invalidate } = await mountBridge();

    act(() => realtime.emit("crm.deals", "deal.updated", { deal: { id: "d1" } }));

    const keysInvalidated = invalidate.mock.calls.map(([filters]) => filters?.queryKey);
    expect(keysInvalidated).toContainEqual(keys.deals.detail("d1"));
    expect(keysInvalidated).toContainEqual([...keys.deals.all, "board"]);
    expect(keysInvalidated).toContainEqual([...keys.deals.all, "list"]);
    expect(keysInvalidated).toContainEqual(keys.dashboard);
  });

  it("still refreshes the board and the lists when the payload names no deal", async () => {
    const { realtime, invalidate } = await mountBridge();

    act(() => realtime.emit("crm.deals", "deal.updated", undefined));

    const keysInvalidated = invalidate.mock.calls.map(([filters]) => filters?.queryKey);
    expect(keysInvalidated).toContainEqual([...keys.deals.all, "board"]);
    expect(keysInvalidated).not.toContainEqual(keys.deals.detail("undefined"));
  });

  it("shows a toast and refreshes the notifications when one arrives", async () => {
    const { realtime, invalidate } = await mountBridge();

    act(() => realtime.emit("users.u7", "notification.created", { title: "Task due", message: "Call Gurpreet Singh" }));

    expect(invalidate.mock.calls.map(([filters]) => filters?.queryKey)).toContainEqual(keys.notifications.all);
    expect(toast).toHaveBeenCalledWith("Task due", { description: "Call Gurpreet Singh" });
  });

  it("ignores another user's notifications", async () => {
    const { realtime } = await mountBridge();

    act(() => realtime.emit("users.u8", "notification.created", { title: "Not for you" }));

    expect(toast).not.toHaveBeenCalled();
  });
});
