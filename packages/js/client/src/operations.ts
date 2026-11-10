/**
 * Operations: long work a user started and follows (an import, an export, a
 * bulk change). Navree's queue module serves them at `/api/operations`:
 * `GET /{id}`, `DELETE /{id}` (cancel) and `GET /{id}/events`, a
 * server-sent event stream with one `operation` event per change that ends
 * with the finished state.
 */
import { NavreeError } from "./errors.js";

export type OperationStatus = "pending" | "running" | "succeeded" | "failed" | "cancelled";

export interface OperationProgress {
  done: number;
  total: number | null;
  percent: number | null;
  note: string | null;
}

/** An operation as the API sends it. */
export interface Operation<Result = unknown> {
  id: string;
  kind: string;
  status: OperationStatus;
  progress: OperationProgress | null;
  result: Result | null;
  error: string | null;
  owner_id: string | null;
  workspace_id: string | null;
  batch_id: string | null;
  created_at: string;
  started_at: string | null;
  heartbeat_at: string | null;
  finished_at: string | null;
  updated_at: string;
}

/** True once an operation has succeeded, failed or been cancelled: it never changes again. */
export function isFinished(operation: Pick<Operation, "status">): boolean {
  return operation.status === "succeeded" || operation.status === "failed" || operation.status === "cancelled";
}

/** One server-sent event. */
export interface ServerSentEvent {
  event: string;
  data: string;
  id?: string;
}

/**
 * The events of a `text/event-stream` body, as they arrive. Follows the
 * HTML event-stream format: `event:`, `data:` (several lines joined with
 * `\n`), `id:`, comments (`:`) skipped, an event ends at a blank line.
 */
export async function* readEventStream(
  body: ReadableStream<Uint8Array>,
  signal?: AbortSignal,
): AsyncGenerator<ServerSentEvent> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let event = "";
  let data: string[] = [];
  let id: string | undefined;

  const dispatch = (): ServerSentEvent | undefined => {
    if (data.length === 0) {
      event = "";
      return undefined;
    }
    const out: ServerSentEvent = { event: event || "message", data: data.join("\n") };
    if (id !== undefined) out.id = id;
    event = "";
    data = [];
    return out;
  };

  const line = (text: string): ServerSentEvent | undefined => {
    if (text === "") return dispatch();
    if (text.startsWith(":")) return undefined;
    const colon = text.indexOf(":");
    const field = colon === -1 ? text : text.slice(0, colon);
    let value = colon === -1 ? "" : text.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") event = value;
    else if (field === "data") data.push(value);
    else if (field === "id" && !value.includes("\0")) id = value;
    return undefined;
  };

  try {
    while (true) {
      if (signal?.aborted) throw signal.reason ?? new DOMException("The operation was aborted.", "AbortError");
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let newline: number;
      while ((newline = buffer.search(/\r\n|\r|\n/)) !== -1) {
        const width = buffer[newline] === "\r" && buffer[newline + 1] === "\n" ? 2 : 1;
        // A lone \r at the end of a chunk may be the first half of \r\n: wait for more.
        if (buffer[newline] === "\r" && newline === buffer.length - 1) break;
        const text = buffer.slice(0, newline);
        buffer = buffer.slice(newline + width);
        const out = line(text);
        if (out) yield out;
      }
    }
    buffer += decoder.decode();
    if (buffer !== "") {
      const out = line(buffer.replace(/\r$/, ""));
      if (out) yield out;
    }
    const last = dispatch();
    if (last) yield last;
  } finally {
    reader.releaseLock();
  }
}

/** Sends a request: `client.raw` from `createNavreeClient`, or `fetch` with a base URL. */
export type OperationFetch = (path: string, init?: RequestInit) => Promise<Response>;

export interface FollowOperationOptions<Result> {
  /** Sends the requests. Default: `fetch` on the page's own origin (a backend-for-frontend). */
  fetch?: OperationFetch;
  /** Where the routes are mounted. Default `/api/operations`. */
  basePath?: string;
  /** Called with the operation on every change, the first state included. */
  onUpdate?: (operation: Operation<Result>) => void;
  /** Stops following (rejects with the abort reason). */
  signal?: AbortSignal;
  /** Reconnects in a row without a new event before giving up. Default 5. */
  retries?: number;
  /** The wait before reconnect `n` is `n × retryDelayMs`. Default 1000. */
  retryDelayMs?: number;
}

const defaultFetch: OperationFetch = (path, init) => globalThis.fetch(path, init);

async function failure(response: Response): Promise<NavreeError> {
  const text = await response.text().catch(() => "");
  let body: unknown = text;
  try {
    body = JSON.parse(text);
  } catch {
    // not JSON
  }
  return NavreeError.fromResponse(response, body);
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (ms <= 0) return resolve();
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener(
      "abort",
      () => {
        clearTimeout(timer);
        reject(signal.reason);
      },
      { once: true },
    );
  });
}

/**
 * Follows an operation over its event stream until it finishes, and
 * returns the finished operation. Reads the stream with `fetch` (not
 * `EventSource`), so it works through a backend-for-frontend and with
 * bearer tokens; a dropped stream is resumed with `Last-Event-ID`.
 */
export async function followOperation<Result = unknown>(
  id: string,
  options: FollowOperationOptions<Result> = {},
): Promise<Operation<Result>> {
  const send = options.fetch ?? defaultFetch;
  const base = (options.basePath ?? "/api/operations").replace(/\/+$/, "");
  const retries = options.retries ?? 5;
  const retryDelayMs = options.retryDelayMs ?? 1000;
  const { signal } = options;
  let lastEventId: string | undefined;
  let attempt = 0;

  while (true) {
    if (signal?.aborted) throw signal.reason ?? new DOMException("The operation was aborted.", "AbortError");
    const headers: Record<string, string> = { accept: "text/event-stream" };
    if (lastEventId !== undefined) headers["last-event-id"] = lastEventId;
    const response = await send(`${base}/${encodeURIComponent(id)}/events`, { headers, signal, cache: "no-store" });
    if (!response.ok) throw await failure(response);

    if (response.body) {
      for await (const event of readEventStream(response.body, signal)) {
        if (event.event !== "operation") continue;
        const operation = JSON.parse(event.data) as Operation<Result>;
        if (event.id !== undefined) lastEventId = event.id;
        attempt = 0;
        options.onUpdate?.(operation);
        if (isFinished(operation)) return operation;
      }
    }

    attempt += 1;
    if (attempt > retries) {
      throw new NavreeError({
        statusCode: 0,
        message: "The operation's event stream closed before it finished.",
        error: "Stream Closed",
        code: "stream_closed",
      });
    }
    await sleep(attempt * retryDelayMs, signal);
  }
}

/** Cancels an operation (`DELETE /api/operations/{id}`) and returns it. */
export async function cancelOperation<Result = unknown>(
  id: string,
  options: Pick<FollowOperationOptions<Result>, "fetch" | "basePath"> = {},
): Promise<Operation<Result>> {
  const send = options.fetch ?? defaultFetch;
  const base = (options.basePath ?? "/api/operations").replace(/\/+$/, "");
  const response = await send(`${base}/${encodeURIComponent(id)}`, {
    method: "DELETE",
    headers: { accept: "application/json" },
  });
  if (!response.ok) throw await failure(response);
  return (await response.json()) as Operation<Result>;
}

/** Loads an operation (`GET /api/operations/{id}`). */
export async function getOperation<Result = unknown>(
  id: string,
  options: Pick<FollowOperationOptions<Result>, "fetch" | "basePath"> = {},
): Promise<Operation<Result>> {
  const send = options.fetch ?? defaultFetch;
  const base = (options.basePath ?? "/api/operations").replace(/\/+$/, "");
  const response = await send(`${base}/${encodeURIComponent(id)}`, { headers: { accept: "application/json" } });
  if (!response.ok) throw await failure(response);
  return (await response.json()) as Operation<Result>;
}
