"use client";

/**
 * Real time in React: one connection to the API's Pusher-protocol server per
 * app, and `useChannel` to listen on a channel while a component is mounted.
 */
import type { ChannelRef, ConnectRealtimeOptions, RealtimeConnection } from "@navree/client/realtime";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";

interface RealtimeContextValue {
  connection: RealtimeConnection | null;
  state: string;
}

const RealtimeContext = createContext<RealtimeContextValue>({ connection: null, state: "disconnected" });

export interface RealtimeProviderProps {
  /** Where the real-time server is, for pusher-js (`NEXT_PUBLIC_*` settings). */
  server?: ConnectRealtimeOptions["server"];
  /** Extra connection options (authorization, the pusher-js class). */
  options?: Omit<ConnectRealtimeOptions, "server">;
  /** Makes the connection; default: pusher-js through `connectRealtime`. Tests pass `createMemoryRealtime`. */
  connect?: () => RealtimeConnection | Promise<RealtimeConnection>;
  /** `false` keeps the socket closed (signed out, real time turned off). Default true. */
  enabled?: boolean;
  children: ReactNode;
}

/**
 * Opens the connection after the first render (the browser only) and closes
 * it on unmount. Real time is optional: if the socket can't connect, the app
 * keeps working and queries still refetch.
 */
export function RealtimeProvider({ server, options, connect, enabled = true, children }: RealtimeProviderProps) {
  const [value, setValue] = useState<RealtimeContextValue>({ connection: null, state: "disconnected" });
  const connectRef = useRef(connect);
  connectRef.current = connect;
  const serverKey = server ? `${server.key}@${server.wsHost}:${server.wsPort}:${server.forceTLS ? "tls" : ""}` : "";

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    let connection: RealtimeConnection | null = null;
    let stopState: (() => void) | undefined;

    const open = async () => {
      try {
        const made = connectRef.current
          ? await connectRef.current()
          : (await import("@navree/client/realtime")).connectRealtime({ ...(options ?? {}), server: server! });
        if (cancelled) {
          made.disconnect();
          return;
        }
        connection = made;
        stopState = made.onStateChange((state) => setValue({ connection: made, state }));
        setValue({ connection: made, state: made.state });
      } catch (error) {
        console.info("[navree] Real time disabled:", error);
      }
    };
    void open();

    return () => {
      cancelled = true;
      stopState?.();
      connection?.disconnect();
      setValue({ connection: null, state: "disconnected" });
    };
    // The connection is rebuilt only when the server or `enabled` changes.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, serverKey]);

  return <RealtimeContext.Provider value={value}>{children}</RealtimeContext.Provider>;
}

/** The app's connection (`null` until it is open, or when real time is off). */
export function useRealtime(): RealtimeConnection | null {
  return useContext(RealtimeContext).connection;
}

/** The socket's state: `connecting`, `connected`, `unavailable`, `failed` or `disconnected`. */
export function useRealtimeState(): string {
  return useContext(RealtimeContext).state;
}

/**
 * Calls `handler` with every `event` on `channel` while the component is
 * mounted. A bare channel name is private (`crm.deals` → `private-crm.deals`).
 * The handler can change on every render without resubscribing; pass `null`
 * as the channel to listen to nothing (no user yet).
 *
 * ```tsx
 * useChannel("crm.deals", "deal.updated", () => queryClient.invalidateQueries({ queryKey: deals.all }));
 * ```
 */
export function useChannel<T = unknown>(channel: ChannelRef | null | undefined, event: string, handler: (data: T) => void): void {
  const connection = useRealtime();
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  const key = channel === null || channel === undefined ? null : typeof channel === "string" ? channel : `public:${channel.public}`;

  useEffect(() => {
    if (!connection || channel === null || channel === undefined) return;
    return connection.listen<T>(channel, event, (data) => handlerRef.current(data));
    // `channel` is captured through `key`, which changes exactly when it does.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [connection, key, event]);
}
