import type { Metadata } from "next";
import Link from "next/link";

import { InlineMarkdown } from "@/components/inline-markdown";
import { changelog, commitUrl, formatDay } from "@/lib/changelog";
import { changelogFeed, pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "Changelog: every Navree release",
  description: `Every ${site.name} release, newest first, with a link to its release notes. ${site.name} 1.0 is the first release.`,
  path: "/changelog/",
});

const label = { feat: "Feature", fix: "Fix" } as const;

/** `/changelog/`: every release, newest first (`src/lib/changelog.ts`). */
export default function ChangelogPage() {
  const days = changelog();
  const total = days.reduce((n, day) => n + day.changes.length, 0);
  return (
    <>
      <header className="container-site pt-14 pb-10 sm:pt-20 lg:pt-24">
        <p className="eyebrow">Changelog</p>
        <h1 className="mt-7 max-w-[18ch] text-display-xl font-semibold text-balance text-fg">What changed in {site.name}.</h1>
        <p className="mt-7 max-w-[40rem] text-lg leading-relaxed text-pretty text-fg-2">
          Every release, newest first. {site.name} 1.0 is the first release; each release from now on is listed here with
          what it adds and fixes.
        </p>
        <p className="mt-5 max-w-[40rem] text-[0.9375rem] leading-relaxed text-fg-3">
          {total} {total === 1 ? "release" : "releases"} so far.{" "}
          <a href={changelogFeed.url} className="link">
            Follow it with the Atom feed
          </a>
          .
        </p>
        {days.length > 1 ? (
          <nav aria-label="Days" className="mt-8">
            <ul className="flex flex-wrap gap-2">
              {days.slice(0, 14).map((day) => (
                <li key={day.day}>
                  <a
                    href={`#${day.day}`}
                    className="inline-flex h-8 items-center rounded-lg border border-line bg-ink-1 px-3 text-sm text-fg-1 transition-colors hover:border-line-2 hover:bg-ink-2"
                  >
                    {formatDay(day.day)}
                    <span className="ml-2 font-mono text-xs text-fg-3 numeric">{day.changes.length}</span>
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        ) : null}
      </header>

      <div className="container-site pb-24">
        {days.length === 0 ? (
          <p className="border-t border-line pt-8 text-fg-2">
            The commit history is not available in this build. The changes are on{" "}
            <a href={`${site.repoUrl}/commits/${site.repoBranch}`} className="link">
              GitHub
            </a>
            .
          </p>
        ) : null}
        {days.map((day) => (
          <section key={day.day} id={day.day} aria-labelledby={`day-${day.day}`} className="scroll-mt-(--header-h) border-t border-line py-10 lg:grid lg:grid-cols-12 lg:gap-12">
            <h2 id={`day-${day.day}`} className="text-display-sm font-semibold text-fg lg:col-span-3">
              <time dateTime={day.day}>{formatDay(day.day)}</time>
              <span className="mt-2 block font-sans text-sm font-normal text-fg-3">
                {day.changes.length} {day.changes.length === 1 ? "change" : "changes"}
              </span>
            </h2>
            <ul className="mt-6 lg:col-span-9 lg:mt-0">
              {day.changes.map((change) => (
                <li key={change.hash} className="flex flex-col gap-1 border-t border-line py-3 first:border-t-0 first:pt-0 sm:flex-row sm:gap-5">
                  <span className="flex shrink-0 items-baseline gap-2 font-mono text-xs sm:w-40">
                    <span className={change.type === "feat" ? "text-accent" : "text-fg-2"}>{label[change.type]}</span>
                    {change.scope ? <span className="truncate text-fg-3">{change.scope}</span> : null}
                  </span>
                  <span className="min-w-0 text-[0.9375rem] leading-relaxed text-pretty text-fg-1">
                    <InlineMarkdown text={change.subject} />{" "}
                    <a
                      href={commitUrl(change.hash)}
                      className="whitespace-nowrap font-mono text-xs text-fg-3 transition-colors hover:text-accent"
                      aria-label={`Commit ${change.hash.slice(0, 7)} on GitHub`}
                    >
                      {change.hash.slice(0, 7)}
                    </a>
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <p className="border-t border-line pt-8 text-sm text-fg-3">
          Looking for what 1.0 ships? Read the{" "}
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
