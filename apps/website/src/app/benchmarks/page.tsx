import type { Metadata } from "next";
import Link from "next/link";

import { ArrowRightIcon } from "@/components/icons";

/**
 * /benchmarks/ moved to /performance/. The site is a static export, so this
 * is a plain HTML redirect: a meta refresh, a canonical link and a visible
 * link for anyone the refresh doesn't move.
 */
export const metadata: Metadata = {
  title: "Moved to Performance",
  description: "Navree's measurements now live at /performance/.",
  alternates: { canonical: "/performance/" },
  robots: { index: false, follow: true },
};

export default function BenchmarksRedirect() {
  return (
    <div className="container-site flex flex-1 flex-col justify-center py-24">
      {/* React hoists this into <head>. */}
      <meta httpEquiv="refresh" content="0; url=/performance/" />
      <p className="eyebrow">Moved</p>
      <h1 className="mt-5 text-display-md font-semibold text-fg">This page is now Performance.</h1>
      <p className="mt-4 max-w-xl text-lg leading-relaxed text-fg-2">
        Navree&apos;s measurements, the workload, the method and how to reproduce them are at{" "}
        <Link href="/performance/" className="link">
          /performance/
        </Link>
        .
      </p>
      <Link
        href="/performance/"
        className="group mt-8 inline-flex h-10 w-fit items-center gap-2 rounded-full bg-accent pr-1.5 pl-4 text-sm font-semibold text-accent-ink transition-shadow duration-200 hover:shadow-glow active:scale-[0.98]"
      >
        Go to Performance
        <span
          aria-hidden
          className="inline-flex size-7 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
        >
          <ArrowRightIcon className="size-3.5" />
        </span>
      </Link>
    </div>
  );
}
