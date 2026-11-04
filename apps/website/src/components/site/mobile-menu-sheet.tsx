"use client";

import { ArrowRightIcon, MenuIcon } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useState } from "react";

import { GitHubIcon } from "@/components/icons";
import { Button } from "@/components/ui/button";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import { site } from "@site";

import { Logo } from "./logo";
import { isCurrent } from "./nav-link";

/**
 * The mobile navigation sheet with its trigger. Loaded by `MobileMenu` the
 * first time the menu button is used, so pages don't ship the dialog code
 * up front; it mounts open.
 */
export function MobileMenuSheet({ className, defaultOpen = false }: { className?: string; defaultOpen?: boolean }) {
  const pathname = usePathname() ?? "";
  const [open, setOpen] = useState(defaultOpen);

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger asChild>
        <Button variant="ghost" size="icon-sm" className={cn("text-fg-1", className)} aria-label="Open the menu">
          <MenuIcon className="size-5" />
        </Button>
      </SheetTrigger>
      <SheetContent side="right" className="gap-0 p-0">
        <div className="flex h-(--header-h) shrink-0 items-center border-b border-line px-5">
          <Logo />
        </div>
        <SheetTitle className="sr-only">Menu</SheetTitle>
        <SheetDescription className="sr-only">The main navigation of {site.domain}.</SheetDescription>
        <nav aria-label="Main" className="flex-1 overflow-y-auto px-3 py-4">
          <ul className="flex flex-col">
            {site.nav.map((item) => {
              const current = isCurrent(pathname, item.match);
              return (
                <li key={item.href}>
                  <SheetClose asChild>
                    <Link
                      href={item.href}
                      prefetch={false}
                      aria-current={current ? "page" : undefined}
                      className={cn(
                        "flex items-center justify-between rounded-xl px-3 py-3 font-display text-[1.375rem] font-semibold tracking-[-0.02em] transition-colors",
                        current ? "bg-ink-2 text-fg" : "text-fg-1 hover:bg-ink-2 hover:text-fg",
                      )}
                    >
                      {item.label}
                      {current ? <span aria-hidden className="size-1.5 rounded-full bg-accent shadow-glow-sm" /> : null}
                    </Link>
                  </SheetClose>
                </li>
              );
            })}
          </ul>
        </nav>
        <div className="flex flex-col gap-2 border-t border-line p-5">
          <SheetClose asChild>
            <Button asChild size="lg" className="w-full">
              <Link href={site.getStarted} prefetch={false}>
                Get started
                <ArrowRightIcon aria-hidden />
              </Link>
            </Button>
          </SheetClose>
          <Button asChild variant="outline" size="lg" className="w-full">
            <a href={site.repoUrl}>
              <GitHubIcon className="size-4" />
              GitHub
            </a>
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
