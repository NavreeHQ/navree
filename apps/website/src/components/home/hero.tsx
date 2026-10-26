import Link from "next/link";

import { GitHubIcon } from "@/components/icons";
import { CopyButton } from "@/components/mdx/copy-button";
import { site } from "@site";

import { HullDrawing } from "./hull-drawing";

/** The tagline, one clause per line: "Laravel's completeness," … */
function promiseLines(tagline: string): string[] {
  return tagline.split(/(?<=,)\s+/);
}

export function Hero() {
  const commands = [site.install.cli, site.install.newApp];
  return (
    <section aria-labelledby="hero-title" className="relative overflow-hidden border-b border-line">
      <div className="mx-auto grid max-w-[90rem] items-center gap-12 px-4 pt-16 pb-20 sm:px-6 sm:pt-20 lg:grid-cols-[minmax(0,1fr)_minmax(0,26rem)] lg:gap-10 lg:px-8 lg:pt-24 lg:pb-28 xl:grid-cols-[minmax(0,1fr)_minmax(0,36rem)] xl:gap-14">
        <div className="min-w-0">
          <h1
            id="hero-title"
            className="text-[2.6rem] leading-[1.02] font-semibold tracking-[-0.045em] text-fg sm:text-[3.5rem] lg:text-[3rem] xl:text-[4rem]"
          >
            {promiseLines(site.tagline).map((line) => (
              <span key={line} className="block">
                {line}
              </span>
            ))}
          </h1>
          <p className="mt-7 max-w-[36rem] text-lg leading-relaxed text-pretty text-fg-2">{site.description}</p>

          <div className="mt-9 flex flex-wrap items-center gap-3">
            <Link href="/docs/getting-started/installation/" className="btn btn-primary">
              Get started
            </Link>
            <Link href="/benchmarks/" className="btn btn-secondary">
              Benchmarks
            </Link>
            <a href={site.repoUrl} className="btn text-fg-2 hover:text-fg">
              <GitHubIcon className="size-[1.125rem]" />
              GitHub
            </a>
          </div>

          <div className="terminal mt-9 flex max-w-[41rem] items-start justify-between gap-3 py-3 pr-2 pl-4" data-copy-root>
            <pre className="min-w-0 py-1 text-[0.875rem] leading-7 whitespace-pre-wrap text-fg">
              <code>
                {commands.map((command) => (
                  <span key={command} className="terminal-line block">
                    {command}
                  </span>
                ))}
              </code>
            </pre>
            <CopyButton text={commands.join("\n")} label="Copy the install commands" className="shrink-0" />
          </div>
        </div>

        <div className="relative mx-auto w-full max-w-[34rem] lg:max-w-none">
          <HullDrawing />
        </div>
      </div>
    </section>
  );
}
