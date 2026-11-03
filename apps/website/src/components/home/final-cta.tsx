import Link from "next/link";
import { Fragment } from "react";

import { GitHubIcon } from "@/components/icons";
import { FlickeringGrid } from "@/components/magicui/flickering-grid";
import { CopyButton } from "@/components/mdx/copy-button";
import { site } from "@site";

import { PrimaryCta, SecondaryCta } from "./parts";

/** A command with line-break opportunities after each `/`, so a long URL wraps at a path segment. */
function breakable(command: string) {
  const parts = command.split("/");
  return parts.map((part, i) => (
    <Fragment key={i}>
      {part}
      {i < parts.length - 1 ? (
        <>
          /<wbr />
        </>
      ) : null}
    </Fragment>
  ));
}

/** The closing call to action: the install commands and the way in. */
export function FinalCta() {
  const commands = [site.install.cli, site.install.newApp];
  return (
    <section aria-labelledby="start" className="cv-auto relative overflow-hidden border-t border-line">
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <FlickeringGrid
          squareSize={3}
          gridGap={6}
          flickerChance={0.18}
          maxOpacity={0.22}
          className="size-full [mask-image:radial-gradient(ellipse_55%_70%_at_50%_100%,black,transparent_75%)]"
        />
      </div>

      <div className="container-site section-y relative flex flex-col items-center text-center">
        <p className="eyebrow">Get started</p>
        <h2 id="start" className="mt-5 max-w-[16ch] text-display-xl font-semibold text-balance text-fg">
          Build the product, not the plumbing.
        </h2>
        <p className="mt-6 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
          Install the CLI and create an app: a working API with auth, workspaces, OpenAPI docs and tests, ready to change. Or
          bring{" "}
          <Link href="/docs/getting-started/bring-your-product/" className="link">
            a product
          </Link>{" "}
          or{" "}
          <Link href="/docs/getting-started/bring-your-design/" className="link">
            a design
          </Link>{" "}
          you already have.
        </p>

        <div
          className="mt-10 flex w-full max-w-2xl items-start gap-2 rounded-2xl border border-line-2 bg-ink-1/90 py-2.5 pr-2 pl-4 text-left shadow-bezel sm:pl-5"
          data-copy-root
        >
          <pre className="min-w-0 flex-1 py-1.5 font-mono text-[0.8125rem] leading-7 whitespace-pre-wrap text-fg sm:text-[0.875rem]">
            <code>
              {commands.map((command) => (
                <span
                  key={command}
                  className="block [overflow-wrap:anywhere] pl-[calc(1ch+0.75rem)] -indent-[calc(1ch+0.75rem)] before:mr-3 before:text-fg-3 before:content-['$']"
                >
                  {breakable(command)}
                </span>
              ))}
            </code>
          </pre>
          <CopyButton text={commands.join("\n")} label="Copy the install commands" className="shrink-0" />
        </div>

        <div className="mt-8 flex flex-wrap items-center justify-center gap-3">
          <PrimaryCta href={site.getStarted}>Get started</PrimaryCta>
          <SecondaryCta href="/docs/">Read the docs</SecondaryCta>
          <a
            href={site.repoUrl}
            className="inline-flex h-12 items-center gap-2 rounded-full px-5 text-base font-medium text-fg-2 transition-colors hover:bg-ink-2 hover:text-fg"
          >
            <GitHubIcon className="size-[1.125rem]" />
            GitHub
          </a>
        </div>
        <p className="mt-8 text-sm text-fg-3">
          Rust {site.rustVersion} or newer · {site.license}
        </p>
      </div>
    </section>
  );
}
