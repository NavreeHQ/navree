/** Build-time public configuration (NEXT_PUBLIC_* is inlined by Next.js). */
export const env = {
  /** `NEXT_PUBLIC_API_MOCKS=1` serves the API from MSW in the browser. Default: real API. */
  mocks: process.env.NEXT_PUBLIC_API_MOCKS === "1",
  /** `NEXT_PUBLIC_REALTIME=0` turns the WebSocket connection off. */
  realtime: process.env.NEXT_PUBLIC_REALTIME !== "0",
  pusherKey: process.env.NEXT_PUBLIC_PUSHER_KEY ?? "navree-crm-key",
  wsHost: process.env.NEXT_PUBLIC_WS_HOST ?? "127.0.0.1",
  wsPort: Number(process.env.NEXT_PUBLIC_WS_PORT ?? 8080),
} as const;
