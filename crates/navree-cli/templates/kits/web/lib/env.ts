/** Build-time public configuration (Next.js inlines NEXT_PUBLIC_* values). */
export const env = {
  /** The API's WebSocket server (Pusher protocol): `realtime.key` and `http.addr` of the API. */
  realtime: {
    key: process.env.NEXT_PUBLIC_REALTIME_KEY ?? "",
    host: process.env.NEXT_PUBLIC_REALTIME_HOST ?? "127.0.0.1",
    port: Number(process.env.NEXT_PUBLIC_REALTIME_PORT ?? 8080),
    tls: process.env.NEXT_PUBLIC_REALTIME_TLS === "true",
  },
} as const;
