"use client";

import { usePathname } from "next/navigation";
import { useEffect, useRef } from "react";

import { CloseIcon, MenuIcon } from "@/components/icons";

import { Sidebar, type SidebarNav } from "./sidebar";

const normalise = (path: string) => (path.endsWith("/") ? path : `${path}/`);

/**
 * Below the `lg` breakpoint the sidebar lives in a drawer, opened from a
 * bar under the header that names the current page.
 */
export function MobileDocsNav({ nav }: { nav: SidebarNav }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const pathname = normalise(usePathname() ?? "/");

  const current = nav.sections
    .flatMap((section) => section.pages.map((page) => ({ section, page })))
    .find(({ page }) => normalise(page.href) === pathname);

  useEffect(() => {
    dialog.current?.close();
  }, [pathname]);

  function open() {
    const el = dialog.current;
    if (!el) return;
    el.showModal();
    // Start the list at the current page rather than at the top.
    el.querySelector('[aria-current="page"]')?.scrollIntoView({ block: "center" });
  }

  return (
    <div className="sticky top-(--header-h) z-30 -mx-4 border-b border-line bg-ink-0/90 px-4 backdrop-blur-md sm:-mx-6 sm:px-6 lg:hidden">
      <button
        type="button"
        onClick={open}
        className="flex h-12 w-full cursor-pointer items-center gap-3 text-left text-sm"
        aria-haspopup="dialog"
      >
        <span className="inline-flex size-8 shrink-0 items-center justify-center rounded-full border border-line-2 bg-ink-1">
          <MenuIcon className="size-4 text-fg-1" />
        </span>
        <span className="sr-only">Open the documentation menu. Current page: </span>
        <span className="min-w-0 truncate">
          {current ? (
            <>
              <span className="text-fg-3">{current.section.title}</span>
              <span className="mx-1.5 text-fg-3" aria-hidden>
                /
              </span>
              <span className="text-fg">{current.page.title}</span>
            </>
          ) : (
            <span className="text-fg">Documentation</span>
          )}
        </span>
      </button>
      <dialog
        ref={dialog}
        aria-label="Documentation menu"
        className="docs-drawer"
        onClick={(event) => {
          if (event.target === dialog.current) dialog.current?.close();
        }}
      >
        <div className="flex h-full flex-col">
          <div className="flex h-14 shrink-0 items-center justify-between border-b border-line px-5">
            <span className="font-mono text-eyebrow text-fg-2 uppercase">Documentation</span>
            <button
              type="button"
              onClick={() => dialog.current?.close()}
              className="-mr-2 inline-flex size-10 cursor-pointer items-center justify-center rounded-full text-fg-2 transition-colors hover:bg-ink-2 hover:text-fg"
              aria-label="Close the menu"
            >
              <CloseIcon className="size-5" />
            </button>
          </div>
          <div className="scroll-thin min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-4">
            <Sidebar nav={nav} onNavigate={() => dialog.current?.close()} />
          </div>
        </div>
      </dialog>
    </div>
  );
}
