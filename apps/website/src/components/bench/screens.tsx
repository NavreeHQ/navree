import Link from "next/link";

import { fmtMs, indian } from "@/lib/format";
import { boardFigures, screenMethod, screenTables } from "@/lib/screens";

/** `2026-11-04` as `4 November 2026`, without a time zone moving the day. */
function day(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
}

/** Bytes as kilobytes of 1,000, one decimal: 65,792 → "65.8 KB". */
const kb = (bytes: number) => `${(bytes / 1000).toFixed(1)} KB`;

const th = "px-4 py-2.5 font-normal";

/**
 * The screens of the example apps, measured at their full size: the store at
 * two lakh orders and the inbox at a lakh conversations. Each figure is read
 * from the file that owns it (`src/lib/screens.ts`), so the page cannot say
 * what a measurement did not.
 */
export function Screens() {
  const apps = screenTables();
  const method = screenMethod();
  const board = boardFigures();
  return (
    <div className="space-y-12">
      {apps.map((app) => (
        <div key={app.app}>
          <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
            <h3 className="font-display text-display-sm font-semibold text-fg">{app.app}</h3>
            <p className="text-sm text-fg-3">
              Measured {day(app.measured)} ·{" "}
              <Link href={app.href} className="link">
                Read about the app
              </Link>
            </p>
          </div>
          <div className="mt-4 overflow-x-auto rounded-xl border border-line">
            <table className="w-full min-w-[40rem] text-left text-sm">
              <caption className="sr-only">
                {app.app}: how long each screen takes to paint, and how much it loads
              </caption>
              <thead className="border-b border-line font-mono text-[0.75rem] tracking-[0.06em] text-fg-3 uppercase">
                <tr>
                  <th scope="col" className={th}>
                    Screen
                  </th>
                  <th scope="col" className={th}>
                    Behind it
                  </th>
                  <th scope="col" className={`${th} text-right`}>
                    Laptop
                  </th>
                  <th scope="col" className={`${th} text-right`}>
                    Phone
                  </th>
                  <th scope="col" className={`${th} text-right`}>
                    Loaded
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {app.screens.map((screen) => (
                  <tr key={screen.name}>
                    <th scope="row" className="px-4 py-2.5 font-normal text-fg-1">
                      {screen.name}
                    </th>
                    <td className="px-4 py-2.5 text-fg-2">{screen.behind}</td>
                    <td className="px-4 py-2.5 text-right font-mono text-fg-1 tabular-nums">{fmtMs(screen.desktopMs)} ms</td>
                    <td className="px-4 py-2.5 text-right font-mono text-fg-1 tabular-nums">
                      {screen.phoneMs === null ? <span className="text-fg-3">not measured</span> : `${indian(screen.phoneMs)} ms`}
                    </td>
                    <td className="px-4 py-2.5 text-right font-mono text-fg-1 tabular-nums">{kb(screen.bytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ))}

      <div>
        <h3 className="font-display text-display-sm font-semibold text-fg">The board</h3>
        <p className="mt-3 max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
          The board holds {indian(board.cards)} cards. Its first page is {indian(board.firstPageBytes)} bytes of HTML,{" "}
          {board.firstPageBrotliKb} KB compressed, and two columns scrolled to their cap of {board.cap} cards hold{" "}
          {indian(board.nodes)} nodes in the page, however long the lists are. It was measured for the size of its page and not
          for a paint ({day(board.measured)}).{" "}
          <Link href="/showcase/board/" className="link">
            Read about the app
          </Link>
          .
        </p>
      </div>

      <p className="max-w-3xl text-[0.9375rem] leading-relaxed text-pretty text-fg-3">
        Paint is the time to the largest thing on the screen, in Chromium on one laptop ({method.machine}) with nothing
        cached. The store was measured on {method.storeMethod}. The phone figure uses a screen {method.phone}. Loaded
        is every file the screen fetched, compressed. These are single-user figures for the screens, not a load test; the
        results above are the load test.
      </p>
    </div>
  );
}
