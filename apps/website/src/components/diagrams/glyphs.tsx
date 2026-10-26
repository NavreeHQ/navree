/**
 * Small line glyphs drawn inside diagram nodes, on a 16 x 16 grid with a
 * 1.25 stroke. Each is plain SVG (no icon font, no runtime requests) and
 * takes its color from the surrounding `stroke` class.
 */
export type GlyphName =
  | "browser"
  | "phone"
  | "server"
  | "database"
  | "module"
  | "container"
  | "broker"
  | "event"
  | "queue"
  | "broadcast"
  | "shield"
  | "document"
  | "code"
  | "lock"
  | "users"
  | "layers"
  | "route"
  | "request"
  | "trace"
  | "worker"
  | "funnel"
  | "braces"
  | "send"
  | "terminal";

const paths: Record<GlyphName, string> = {
  browser: "M2.5 3.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Z M1.5 6.5h13 M3.75 5h.01 M5.25 5h.01",
  phone: "M5 1.5h6a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1Z M7 12.25h2",
  server:
    "M2.5 2.5h11a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1Z M2.5 9h11a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1Z M4 4.75h.01 M4 11.25h.01",
  database:
    "M2.5 4c0-1.1 2.46-2 5.5-2s5.5.9 5.5 2-2.46 2-5.5 2-5.5-.9-5.5-2Z M2.5 4v8c0 1.1 2.46 2 5.5 2s5.5-.9 5.5-2V4 M2.5 8c0 1.1 2.46 2 5.5 2s5.5-.9 5.5-2",
  module: "M8 1.75 13.75 5v6L8 14.25 2.25 11V5L8 1.75Z M2.25 5 8 8.25 13.75 5 M8 8.25v6",
  container:
    "M2.5 2.5h4v4h-4z M9.5 2.5h4v4h-4z M2.5 9.5h4v4h-4z M9.5 9.5h4v4h-4z M6.5 4.5h3 M4.5 6.5v3 M11.5 6.5v3 M6.5 11.5h3",
  broker:
    "M8 6.25a1.75 1.75 0 1 1 0 3.5 1.75 1.75 0 0 1 0-3.5Z M8 1.5v4.75 M8 9.75v4.75 M1.5 8h4.75 M9.75 8h4.75 M3 3l2.75 2.75 M10.25 10.25 13 13 M13 3l-2.75 2.75 M5.75 10.25 3 13",
  event: "M9 1.5 3.5 9h4L7 14.5 12.5 7h-4L9 1.5Z",
  queue: "M2.5 3.5h11 M2.5 6.5h11 M2.5 9.5h11 M2.5 12.5h7",
  broadcast:
    "M8 7.25a.75.75 0 1 1 0 1.5.75.75 0 0 1 0-1.5Z M5.17 5.17a4 4 0 0 0 0 5.66 M10.83 5.17a4 4 0 0 1 0 5.66 M3.05 3.05a7 7 0 0 0 0 9.9 M12.95 3.05a7 7 0 0 1 0 9.9",
  shield: "M8 1.75 13 3.5v4.25c0 3.1-2.13 5.38-5 6.5-2.87-1.12-5-3.4-5-6.5V3.5l5-1.75Z M5.75 8 7.4 9.6l3-3.1",
  document: "M4 1.5h5.5L12.5 4.5v9a1 1 0 0 1-1 1h-7.5a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1Z M9.5 1.5v3h3 M5.5 8h5 M5.5 10.75h5",
  code: "M5.5 4.5 2 8l3.5 3.5 M10.5 4.5 14 8l-3.5 3.5 M9 3 7 13",
  lock: "M4 7h8a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1Z M5.25 7V5a2.75 2.75 0 0 1 5.5 0v2 M8 9.75v1.5",
  users:
    "M6 2.75a2.25 2.25 0 1 1 0 4.5 2.25 2.25 0 0 1 0-4.5Z M1.75 13.5c0-2.35 1.9-4.25 4.25-4.25s4.25 1.9 4.25 4.25 M10.5 2.9a2.25 2.25 0 0 1 0 4.2 M12 9.6c1.35.7 2.25 2.1 2.25 3.9",
  layers: "M8 1.75 14.25 5 8 8.25 1.75 5 8 1.75Z M1.75 8 8 11.25 14.25 8 M1.75 11 8 14.25 14.25 11",
  route: "M3.5 2v4.5a2 2 0 0 0 2 2h5a2 2 0 0 1 2 2V14 M3.5 8.5V14 M10.5 12l2 2 2-2",
  request: "M1.5 8h10.5 M8.5 4.5 12 8l-3.5 3.5 M14.5 3v10",
  trace: "M1.5 4h7 M4 8h8 M7 12h7.5 M1.5 2.5v3 M4 6.5v3 M7 10.5v3",
  worker:
    "M8 5.5a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5Z M8 1.5v1.75 M8 12.75v1.75 M1.5 8h1.75 M12.75 8h1.75 M3.4 3.4l1.24 1.24 M11.36 11.36l1.24 1.24 M12.6 3.4l-1.24 1.24 M4.64 11.36 3.4 12.6",
  funnel: "M2 3h12l-4.5 5.25V13l-3 1.5V8.25L2 3Z",
  braces:
    "M5.5 2.5h-.75a1.5 1.5 0 0 0-1.5 1.5v2.25c0 .83-.67 1.75-1.5 1.75.83 0 1.5.92 1.5 1.75V12a1.5 1.5 0 0 0 1.5 1.5h.75 M10.5 2.5h.75a1.5 1.5 0 0 1 1.5 1.5v2.25c0 .83.67 1.75 1.5 1.75-.83 0-1.5.92-1.5 1.75V12a1.5 1.5 0 0 1-1.5 1.5h-.75",
  send: "M14.5 1.5 7 9 M14.5 1.5 10 14.5 7 9 1.5 6l13-4.5Z",
  terminal: "M2.5 2.5h11a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1Z M4.5 6l2 2-2 2 M8 10.5h3.5",
};

/** One glyph, 16 x 16, with its top-left corner at (x, y). */
export function Glyph({ name, x, y, className }: { name: GlyphName; x: number; y: number; className?: string }) {
  return (
    <path
      d={paths[name]}
      transform={`translate(${x} ${y})`}
      fill="none"
      strokeWidth={1.25}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    />
  );
}
