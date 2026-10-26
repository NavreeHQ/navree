import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";

import { GitHubIcon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { site } from "@site";

import { HeaderSearch } from "./header-search";
import { Logo } from "./logo";
import { MobileMenu } from "./mobile-menu";
import { NavLink } from "./nav-link";

export function SiteHeader() {
  return (
    <header className="sticky top-0 z-40 h-(--header-h) border-b border-line bg-ink-0/80 backdrop-blur-xl supports-[backdrop-filter]:bg-ink-0/65">
      <div className="container-site flex h-full items-center gap-3">
        <Link href="/" className="-ml-1 shrink-0 rounded-md px-1 py-1" aria-label={`${site.name} home`}>
          <Logo />
        </Link>
        <nav aria-label="Main" className="ml-5 hidden items-center gap-0.5 lg:flex">
          {site.nav.map((item) => (
            <NavLink key={item.href} href={item.href} match={item.match}>
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-1.5 sm:gap-2">
          <HeaderSearch />
          <a
            href={site.repoUrl}
            className="hidden size-9 items-center justify-center rounded-full text-fg-2 transition-colors hover:bg-ink-2 hover:text-fg sm:inline-flex"
            aria-label={`${site.name} on GitHub`}
          >
            <GitHubIcon className="size-[1.125rem]" />
          </a>
          <Button asChild size="sm" className="hidden h-9 pr-1.5 pl-4 sm:inline-flex">
            <Link href={site.getStarted} className="group">
              Get started
              <span
                aria-hidden
                className="inline-flex size-6 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
              >
                <ArrowRightIcon className="size-3.5" />
              </span>
            </Link>
          </Button>
          <MobileMenu className="lg:hidden" />
        </div>
      </div>
    </header>
  );
}
