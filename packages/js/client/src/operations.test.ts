import { describe, expect, it, vi } from "vitest";
import { NavreeError } from "./errors.js";
import { cancelOperation, followOperation, isFinished, readEventStream, type Operation } from "./operations.js";

const encoder = new TextEncoder();

/** A response body that arrives in the given chunks (split anywhere, even mid-line). */
function stream(chunks: string[]): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
}

const op = (status: Operation["status"], done: number, extra: Partial<Operation> = {}): Operation => ({
  id: "op1",
  kind: "contacts.import",
  status,
  progress: { done, total: 4, percent: done * 25, note: null },
  result: status === "succeeded" ? { imported: 4 } : null,
  error: null,
  owner_id: "u1",
  workspace_id: null,
  batch_id: null,
  created_at: "2026-10-27T09:12:44.120Z",
  started_at: null,
  heartbeat_at: null,
  finished_at: status === "succeeded" ? "2026-10-27T09:12:50.000Z" : null,
  updated_at: "2026-10-27T09:12:47.415Z",
  ...extra,
});

const sse = (operation: Operation, id: number) => `event: operation\nid: ${id}\ndata: ${JSON.stringify(operation)}\n\n`;

describe("readEventStream", () => {
  it("parses events split across chunks, with CRLF, comments and multi-line data", async () => {
    const events = [];
    for await (const event of readEventStream(
      stream([": keep-alive\r\n\r\nevent: op", "eration\r\nid: 4\r\ndata: {\"a\":\r\ndata: 1}\r\n\r\n", "data: plain\n\n", "event: x\ndata: last"]),
    )) {
      events.push(event);
    }

    // As in the HTML spec, the last event id carries over to later events until a new `id:`.
    expect(events).toEqual([
      { event: "operation", id: "4", data: '{"a":\n1}' },
      { event: "message", id: "4", data: "plain" },
      { event: "x", id: "4", data: "last" },
    ]);
  });
});

describe("followOperation", () => {
  it("reads the operation's event stream until it finishes, reporting every change", async () => {
    const fetch = vi.fn(async (_path: string, _init?: RequestInit) =>
      new Response(stream([sse(op("running", 1), 3), sse(op("running", 2), 4), sse(op("succeeded", 4), 7)]), {
        headers: { "content-type": "text/event-stream" },
      }),
    );
    const updates: number[] = [];

    const finished = await followOperation("op1", { fetch, onUpdate: (o) => updates.push(o.progress?.done ?? -1) });

    expect(finished.status).toBe("succeeded");
    expect(finished.result).toEqual({ imported: 4 });
    expect(updates).toEqual([1, 2, 4]);
    const [path, init] = fetch.mock.calls[0]!;
    expect(path).toBe("/api/operations/op1/events");
    expect(new Headers(init?.headers).get("accept")).toBe("text/event-stream");
  });

  it("reconnects with Last-Event-ID when the stream closes before the end", async () => {
    const fetch = vi
      .fn<(path: string, init?: RequestInit) => Promise<Response>>()
      .mockResolvedValueOnce(new Response(stream([sse(op("running", 1), 3)])))
      .mockResolvedValueOnce(new Response(stream([sse(op("succeeded", 4), 9)])));

    const finished = await followOperation("op1", { fetch, retryDelayMs: 0 });

    expect(finished.status).toBe("succeeded");
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(new Headers(fetch.mock.calls[1]![1]?.headers).get("last-event-id")).toBe("3");
  });

  it("gives up after `retries` reconnects without news", async () => {
    const fetch = vi.fn(async () => new Response(stream([": keep-alive\n\n"])));
    const error = await followOperation("op1", { fetch, retries: 2, retryDelayMs: 0 }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(NavreeError);
    expect((error as NavreeError).code).toBe("stream_closed");
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("throws the API's error (404 for an operation the user may not see)", async () => {
    const fetch = vi.fn(async () =>
      Response.json({ statusCode: 404, message: "Not Found", error: "Not Found", code: "not_found" }, { status: 404 }),
    );
    const error = await followOperation("op1", { fetch }).catch((e: unknown) => e);
    expect((error as NavreeError).code).toBe("not_found");
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("returns at once for an operation that already finished, and honours an abort", async () => {
    const fetch = vi.fn(async () => new Response(stream([sse(op("failed", 2, { error: "1 of 4 jobs failed." }), 5)])));
    const failed = await followOperation("op1", { fetch });
    expect(failed.status).toBe("failed");
    expect(failed.error).toBe("1 of 4 jobs failed.");

    const controller = new AbortController();
    controller.abort();
    await expect(followOperation("op1", { fetch, signal: controller.signal })).rejects.toThrow(/abort/i);
  });

  it("uses another API prefix", async () => {
    const fetch = vi.fn(async (_path: string) => new Response(stream([sse(op("cancelled", 1), 2)])));
    await followOperation("op1", { fetch, basePath: "/v2/operations" });
    expect(fetch.mock.calls[0]![0]).toBe("/v2/operations/op1/events");
  });
});

describe("cancelOperation", () => {
  it("sends DELETE and returns the cancelled operation", async () => {
    const fetch = vi.fn(async (_path: string, _init?: RequestInit) => Response.json(op("cancelled", 1)));
    const cancelled = await cancelOperation("op1", { fetch });
    expect(cancelled.status).toBe("cancelled");
    expect(fetch.mock.calls[0]![0]).toBe("/api/operations/op1");
    expect(fetch.mock.calls[0]![1]?.method).toBe("DELETE");
  });

  it("throws a 403 when the user may see but not cancel it", async () => {
    const fetch = vi.fn(async () =>
      Response.json({ statusCode: 403, message: "Forbidden", error: "Forbidden", code: "forbidden" }, { status: 403 }),
    );
    await expect(cancelOperation("op1", { fetch })).rejects.toMatchObject({ status: 403, code: "forbidden" });
  });
});

describe("isFinished", () => {
  it("is true for succeeded, failed and cancelled", () => {
    expect(isFinished(op("pending", 0))).toBe(false);
    expect(isFinished(op("running", 1))).toBe(false);
    expect(isFinished(op("succeeded", 4))).toBe(true);
    expect(isFinished(op("failed", 1))).toBe(true);
    expect(isFinished(op("cancelled", 1))).toBe(true);
  });
});
