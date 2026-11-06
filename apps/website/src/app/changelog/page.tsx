import type { Metadata } from "next";
import Link from "next/link";

import { ChevronIcon } from "@/components/icons";
import { InlineMarkdown } from "@/components/inline-markdown";
import { changelog, commitUrl, formatDay } from "@/lib/changelog";
import { changelogFeed, pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "Changelog: every Navree release",
  description: `Every ${site.name} release by week, newest first, with a short summary of each week and its changes under the part of ${site.name} they touch. ${site.name} 1.0 is the first release.`,
  path: "/changelog/",
});

const label = { feat: "Feature", fix: "Fix" } as const;

/**
 * `/changelog/`: every release by week, newest first (`src/lib/changelog.ts`).
 * A week opens with its highlight; its changes sit under area headings that
 * start closed, so the page reads as a list of weeks and opens to the detail.
 */
export default function ChangelogPage() {
  const weeks = changelog();
  const releases = weeks.reduce((n, week) => n + week.releases.length, 0);
  return (
    <>
      <header className="container-site pt-14 pb-10 sm:pt-20 lg:pt-24">
        <p className="eyebrow">Changelog</p>
        <h1 className="mt-7 max-w-[18ch] text-display-xl font-semibold text-balance text-fg">What changed in {site.name}.</h1>
        <p className="mt-7 max-w-[40rem] text-lg leading-relaxed text-pretty text-fg-2">
          Every release by week, newest first. {site.name} 1.0 is the first release; each release from now on is listed here with
          what it adds and fixes.
        </p>
        <p className="mt-5 max-w-[40rem] text-[0.9375rem] leading-relaxed text-fg-3">
          {releases} {releases === 1 ? "release" : "releases"} so far.{" "}
          <a href={changelogFeed.url} className="link">
            Follow it with the Atom feed
          </a>
          .
        </p>
        {weeks.length > 1 ? (
          <nav aria-label="Weeks" className="mt-8">
            <ul className="flex flex-wrap gap-2">
              {weeks.slice(0, 14).map((week) => (
                <li key={week.start}>
                  <a
                    href={`#week-${week.start}`}
                    className="inline-flex h-8 items-center rounded-lg border border-line bg-ink-1 px-3 text-sm text-fg-1 transition-colors hover:border-line-2 hover:bg-ink-2"
                  >
                    {formatDay(week.start)}
                    <span className="ml-2 font-mono text-xs text-fg-3 numeric">{week.count}</span>
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        ) : null}
      </header>

      <div className="container-site pb-24">
        {weeks.map((week) => (
          <section
            key={week.start}
            id={`week-${week.start}`}
            aria-labelledby={`week-heading-${week.start}`}
            className="scroll-mt-(--header-h) border-t border-line py-10 lg:grid lg:grid-cols-12 lg:gap-12"
          >
            <h2 id={`week-heading-${week.start}`} className="text-display-sm font-semibold text-fg lg:col-span-3">
              <span className="block font-sans text-sm font-normal text-fg-3">Week of</span>
              <time dateTime={week.start}>{formatDay(week.start)}</time>
              <span className="mt-2 block font-sans text-sm font-normal text-fg-3">
                {week.count} {week.count === 1 ? "change" : "changes"}
              </span>
            </h2>
            <div className="mt-6 min-w-0 lg:col-span-9 lg:mt-0">
              <p className="max-w-2xl text-lg leading-relaxed text-pretty text-fg-1">{week.highlight}</p>
              <ul className="mt-4 flex flex-col gap-1 text-sm text-fg-3">
                {week.releases.map((release) => (
                  <li key={release.version}>
                    <span className="font-medium text-fg-1">
                      {site.name} {release.version}
                    </span>{" "}
                    on <time dateTime={release.day}>{formatDay(release.day)}</time>,{" "}
                    <a
                      href={commitUrl(release.hash)}
                      className="font-mono text-xs transition-colors hover:text-accent"
                      aria-label={`Commit ${release.hash.slice(0, 7)} on GitHub`}
                    >
                      {release.hash.slice(0, 7)}
                    </a>
                  </li>
                ))}
              </ul>

              <div className="mt-8 border-t border-line">
                {week.areas.map((area) => (
                  <details key={area.name} className="group/area border-b border-line">
                    <summary className="flex cursor-pointer list-none items-center justify-between gap-4 py-4 [&::-webkit-details-marker]:hidden">
                      <h3 className="flex items-baseline gap-3 text-base font-semibold text-fg">
                        {area.name}
                        <span className="font-mono text-xs font-normal text-fg-3 numeric">{area.changes.length}</span>
                      </h3>
                      <ChevronIcon aria-hidden className="size-4 shrink-0 text-fg-3 transition-transform duration-200 group-open/area:rotate-90" />
                    </summary>
                    <ul className="pb-4">
                      {area.changes.map((change) => (
                        <li
                          key={change.subject}
                          data-change=""
                          className="flex flex-col gap-1 border-t border-line py-3 first:border-t-0 sm:flex-row sm:gap-5"
                        >
                          <span className="flex shrink-0 items-baseline gap-2 font-mono text-xs sm:w-28">
                            <span className={change.type === "feat" ? "text-accent" : "text-fg-2"}>{label[change.type]}</span>
                            {change.scope ? <span className="truncate text-fg-3">{change.scope}</span> : null}
                          </span>
                          <span className="min-w-0 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">
                            <InlineMarkdown text={change.subject} />{" "}
                            <Link
                              href={change.link.href}
                              className="whitespace-nowrap text-sm text-fg-3 transition-colors hover:text-accent"
                            >
                              {change.link.label}
                            </Link>
                          </span>
                        </li>
                      ))}
                    </ul>
                  </details>
                ))}
              </div>
            </div>
          </section>
        ))}
        <p className="border-t border-line pt-8 text-sm text-fg-3">
          Looking for the full list of what 1.0 ships? Read the{" "}
          <Link href="/docs/prologue/release-notes/" className="link">
            release notes
          </Link>
          .
        </p>
      </div>
      <PageSchema path="/changelog/" metadata={metadata} />
    </>
  );
}
