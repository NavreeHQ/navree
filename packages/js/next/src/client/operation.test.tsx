import type { Operation } from "@navree/client";
import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useOperation } from "./operation.js";

const op = (status: Operation["status"], done: number): Operation => ({
  id: "op1",
  kind: "contacts.import",
  status,
  progress: { done, total: 4, percent: done * 25, note: null },
  result: status === "succeeded" ? { imported: 4 } : null,
  error: null,
  owner_id: "u1",
  workspace_id: null,
  batch_id: null,
  created_at: "2026-10-27T09:00:00Z",
  started_at: null,
  heartbeat_at: null,
  finished_at: null,
  updated_at: "2026-10-27T09:00:00Z",
});

/** An event stream the test pushes events into. */
function controlledStream() {
  const encoder = new TextEncoder();
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const body = new ReadableStream<Uint8Array>({ start: (c) => (controller = c) });
  return {
    body,
    send: (operation: Operation, id: number) =>
      controller.enqueue(encoder.encode(`event: operation\nid: ${id}\ndata: ${JSON.stringify(operation)}\n\n`)),
    close: () => controller.close(),
  };
}

function Progress({ id, fetch, onFinished }: { id: string | null; fetch: (path: string, init?: RequestInit) => Promise<Response>; onFinished?: (o: Operation) => void }) {
  const { operation, finished, error, cancel } = useOperation(id, { fetch, onFinished });
  return (
    <div>
      <p>{error ? `error ${(error as { code?: string }).code ?? error.message}` : operation ? `${operation.status} ${operation.progress?.percent}%` : "waiting"}</p>
      <p>{finished ? "finished" : "working"}</p>
      <button onClick={() => void cancel()}>Cancel</button>
    </div>
  );
}

describe("useOperation", () => {
  it("re-renders on every change until the operation finishes", async () => {
    const stream = controlledStream();
    const fetch = vi.fn(async (_path: string, _init?: RequestInit) => new Response(stream.body));
    const onFinished = vi.fn();
    render(<Progress id="op1" fetch={fetch} onFinished={onFinished} />);
    expect(screen.getByText("waiting")).toBeTruthy();

    act(() => stream.send(op("running", 1), 1));
    await waitFor(() => expect(screen.getByText("running 25%")).toBeTruthy());
    act(() => stream.send(op("succeeded", 4), 2));
    await waitFor(() => expect(screen.getByText("succeeded 100%")).toBeTruthy());

    expect(screen.getByText("finished")).toBeTruthy();
    expect(onFinished).toHaveBeenCalledWith(expect.objectContaining({ status: "succeeded" }));
    expect(fetch.mock.calls[0]![0]).toBe("/api/operations/op1/events");
  });

  it("does nothing without an id", () => {
    const fetch = vi.fn();
    render(<Progress id={null} fetch={fetch} />);
    expect(screen.getByText("waiting")).toBeTruthy();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("shows the API's error", async () => {
    const fetch = vi.fn(async () =>
      Response.json({ statusCode: 404, message: "Not Found", error: "Not Found", code: "not_found" }, { status: 404 }),
    );
    render(<Progress id="op9" fetch={fetch} />);
    await waitFor(() => expect(screen.getByText("error not_found")).toBeTruthy());
  });

  it("cancels the operation", async () => {
    const stream = controlledStream();
    const fetch = vi.fn(async (_path: string, init?: RequestInit) =>
      init?.method === "DELETE" ? Response.json(op("cancelled", 1)) : new Response(stream.body),
    );
    render(<Progress id="op1" fetch={fetch} />);
    act(() => stream.send(op("running", 1), 1));
    await waitFor(() => expect(screen.getByText("running 25%")).toBeTruthy());

    act(() => screen.getByRole("button", { name: "Cancel" }).click());

    await waitFor(() => expect(screen.getByText("cancelled 25%")).toBeTruthy());
    expect(fetch).toHaveBeenCalledWith("/api/operations/op1", expect.objectContaining({ method: "DELETE" }));
  });

  it("stops reading the stream on unmount", async () => {
    const stream = controlledStream();
    let signal: AbortSignal | undefined;
    const fetch = vi.fn(async (_path: string, init?: RequestInit) => {
      signal = init?.signal ?? undefined;
      return new Response(stream.body);
    });
    const { unmount } = render(<Progress id="op1" fetch={fetch} />);
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    unmount();
    expect(signal?.aborted).toBe(true);
  });
});
