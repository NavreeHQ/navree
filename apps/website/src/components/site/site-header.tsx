import Link from "next/link";

import { GitHubIcon } from "@/components/icons";
import { SearchButton } from "@/components/search/search-button";
import { site } from "@site";

import { Logo } from "./logo";
import { NavLink } from "./nav-link";

export function SiteHeader() {
  return (
    <header className="sticky top-0 z-40 h-(--header-h) border-b border-line bg-ink-0/85 backdrop-blur-md supports-[backdrop-filter]:bg-ink-0/70">
      <div className="mx-auto flex h-full max-w-[90rem] items-center gap-3 px-4 sm:gap-6 sm:px-6 lg:px-8">
        <Link href="/" className="-ml-1 shrink-0 rounded-md px-1 py-1" aria-label={`${site.name} home`}>
          <Logo />
        </Link>
        <nav aria-label="Main" className="flex items-center gap-1 text-[0.9375rem] sm:gap-2">
          <NavLink href="/docs/" match="/docs">
            Docs
          </NavLink>
          <NavLink href="/benchmarks/" match="/benchmarks">
            Benchmarks
          </NavLink>
        </nav>
        <div className="ml-auto flex items-center gap-1.5 sm:gap-3">
          <SearchButton />
          <a
            href={site.repoUrl}
            className="inline-flex size-10 items-center justify-center rounded-md text-fg-2 transition-colors hover:text-fg"
            aria-label={`${site.name} on GitHub`}
          >
            <GitHubIcon className="size-5" />
          </a>
        </div>
      </div>
    </header>
  );
}
