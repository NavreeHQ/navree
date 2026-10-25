/** The slice of Laravel Echo the app uses, so the mock bus can stand in for it. */
export interface RealtimeConnection {
  listen(channel: string, event: string, callback: (data: unknown) => void): void;
  leave(channel: string): void;
  disconnect(): void;
}
