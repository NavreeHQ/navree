import type { Metadata } from "next";

import { ArrowLink, PrimaryCta, SecondaryCta } from "@/components/home/parts";
import { GridPattern } from "@/components/magicui/grid-pattern";
import { site } from "@site";

/** The rest of the main navigation: the docs and the homepage are the buttons above it. */
const elsewhere = site.nav.filter((item) => item.href !== "/docs/");

export const metadata: Metadata = {
  title: "Page not found",
  description: "This page isn't here. Search the docs, or start from the homepage.",
  robots: { index: false, follow: true },
};

export default function NotFound() {
  return (
    <section aria-labelledby="not-found-title" className="relative isolate flex flex-1 flex-col overflow-hidden">
      <GridPattern
        width={48}
        height={48}
        squares={[
          [19, 2],
          [23, 4],
          [26, 1],
        ]}
        className="-z-10 [mask-image:radial-gradient(ellipse_65%_70%_at_75%_10%,black_10%,transparent_75%)]"
      />
      <div className="container-site flex flex-1 flex-col justify-center py-24 sm:py-32">
        <p className="eyebrow">404</p>
        <h1 id="not-found-title" className="mt-5 text-display-xl font-semibold text-balance text-fg">
          This page isn&apos;t here.
        </h1>
        <p className="mt-5 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
          The link may be old, or the page may have moved. Search the docs with <kbd className="kbd">/</kbd>, or start
          from one of these.
        </p>
        <div className="mt-10 flex flex-wrap items-center gap-3">
          <PrimaryCta href="/docs/">Read the docs</PrimaryCta>
          <SecondaryCta href="/">Home</SecondaryCta>
        </div>
        <ul className="mt-12 flex flex-wrap gap-x-8 gap-y-3 border-t border-line pt-6 sm:max-w-xl">
          {elsewhere.map((link) => (
            <li key={link.href}>
              <ArrowLink href={link.href}>{link.label}</ArrowLink>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
