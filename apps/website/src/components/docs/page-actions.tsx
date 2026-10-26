"use client";

import { ArrowUpRightIcon, CheckIcon, ChevronDownIcon, CopyIcon, FileTextIcon } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { cn } from "@/lib/utils";

type State = "idle" | "copied" | "failed";

/** The prompt "Open in Claude" and "Open in ChatGPT" start with. */
export function pagePrompt(title: string, markdownUrl: string): string {
  return `Read ${markdownUrl} first. It is the "${title}" page of the Navree docs (Navree is the complete backend framework for Rust). Then answer my questions about it and help me use it in my project.`;
}

/**
 * The actions next to a docs page's title: "Copy page" copies the page's
 * Markdown, and the menu views it as Markdown or opens it in Claude or
 * ChatGPT with a prompt that names its `.md` URL.
 *
 * Copying fetches `markdownPath` (a static file in the export). The
 * clipboard write starts inside the click, with the text as a promise, so
 * Safari allows it; browsers without `ClipboardItem` fetch, then write.
 */
export function PageActions({
  title,
  markdownPath,
  markdownUrl,
  className,
}: {
  title: string;
  /** `/docs/basics/validation.md`, fetched for copying and linked for viewing. */
  markdownPath: string;
  /** The absolute URL the AI assistants are asked to read. */
  markdownUrl: string;
  className?: string;
}) {
  const [state, setState] = useState<State>("idle");
  const cache = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const prompt = encodeURIComponent(pagePrompt(title, markdownUrl));
  const claude = `https://claude.ai/new?q=${prompt}`;
  const chatgpt = `https://chatgpt.com/?q=${prompt}`;

  async function text(): Promise<string> {
    if (cache.current !== null) return cache.current;
    const response = await fetch(markdownPath);
    if (!response.ok) throw new Error(`${markdownPath}: ${response.status}`);
    cache.current = await response.text();
    return cache.current;
  }

  function show(next: State) {
    setState(next);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), 2000);
  }

  async function copy() {
    try {
      if (cache.current === null && typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
        const blob = text().then((value) => new Blob([value], { type: "text/plain" }));
        await navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
      } else {
        await navigator.clipboard.writeText(await text());
      }
      show("copied");
    } catch {
      try {
        await navigator.clipboard.writeText(await text());
        show("copied");
      } catch {
        show("failed");
      }
    }
  }

  const label = state === "copied" ? "Copied" : state === "failed" ? "Copy failed" : "Copy page";

  return (
    <div
      className={cn(
        "inline-flex h-8 shrink-0 items-stretch rounded-full border border-line-2 bg-ink-1 text-[0.8125rem] font-medium text-fg-1 shadow-bezel",
        className,
      )}
    >
      <button
        type="button"
        onClick={copy}
        className="inline-flex cursor-pointer items-center gap-1.5 rounded-l-full pr-2.5 pl-3 transition-colors duration-150 hover:bg-ink-2 hover:text-fg"
      >
        {state === "copied" ? (
          <CheckIcon className="size-3.5 text-accent" strokeWidth={2} aria-hidden />
        ) : (
          <CopyIcon className="size-3.5 text-fg-3" strokeWidth={1.75} aria-hidden />
        )}
        {/* Every label in one cell, so the button keeps its width. */}
        <span className="grid">
          {["Copy page", "Copied", "Copy failed"].map((value) => (
            <span
              key={value}
              aria-hidden={value !== label}
              className={cn("col-start-1 row-start-1 text-left", value === label ? "visible" : "invisible")}
            >
              {value}
            </span>
          ))}
        </span>
        <span className="sr-only" aria-live="polite">
          {state === "copied" ? "Page copied as Markdown" : state === "failed" ? "Could not copy the page" : ""}
        </span>
      </button>
      <span aria-hidden className="my-1.5 w-px bg-line-2" />
      <DropdownMenu.Root modal={false}>
        <DropdownMenu.Trigger
          aria-label="More page actions"
          className="inline-flex w-8 cursor-pointer items-center justify-center rounded-r-full text-fg-3 transition-colors duration-150 hover:bg-ink-2 hover:text-fg data-[state=open]:bg-ink-2 data-[state=open]:text-fg"
        >
          <ChevronDownIcon className="size-3.5" strokeWidth={1.75} />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            align="end"
            sideOffset={8}
            collisionPadding={12}
            className="z-50 w-[17.5rem] origin-(--radix-dropdown-menu-content-transform-origin) animate-in rounded-xl border border-line-2 bg-ink-1 p-1.5 shadow-float fade-in-0 zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95"
          >
            <Item
              icon={<CopyIcon strokeWidth={1.75} />}
              title="Copy page"
              text="The page as Markdown, for any chat or agent"
              onSelect={() => void copy()}
            />
            <Item
              icon={<FileTextIcon strokeWidth={1.75} />}
              title="View as Markdown"
              text="The plain-text page models read"
              href={markdownPath}
            />
            <DropdownMenu.Separator className="mx-2 my-1.5 h-px bg-line" />
            <Item
              icon={<ClaudeMark />}
              title="Open in Claude"
              text="Ask Claude about this page"
              href={claude}
              external
            />
            <Item
              icon={<ChatGptMark />}
              title="Open in ChatGPT"
              text="Ask ChatGPT about this page"
              href={chatgpt}
              external
            />
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  );
}

function Item({
  icon,
  title,
  text,
  href,
  external = false,
  onSelect,
}: {
  icon: ReactNode;
  title: string;
  text: string;
  href?: string;
  external?: boolean;
  onSelect?: () => void;
}) {
  const body = (
    <>
      <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg border border-line bg-ink-2 text-fg-2 transition-colors group-data-[highlighted]:border-line-2 group-data-[highlighted]:text-fg [&_svg]:size-3.5">
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1 text-[0.8125rem] font-medium text-fg">
          {title}
          {external ? <ArrowUpRightIcon className="size-3 text-fg-3" strokeWidth={1.75} aria-label="(opens a new tab)" /> : null}
        </span>
        <span className="block text-xs leading-snug text-fg-3">{text}</span>
      </span>
    </>
  );
  const className =
    "group flex cursor-pointer items-start gap-2.5 rounded-lg px-2 py-2 outline-none transition-colors data-[highlighted]:bg-ink-2";
  if (href) {
    return (
      <DropdownMenu.Item asChild className={className}>
        <a href={href} {...(external ? { target: "_blank", rel: "noreferrer" } : {})}>
          {body}
        </a>
      </DropdownMenu.Item>
    );
  }
  return (
    <DropdownMenu.Item className={className} onSelect={onSelect}>
      {body}
    </DropdownMenu.Item>
  );
}

/** A neutral eight-point spark for Claude (not the brand mark). */
function ClaudeMark() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" aria-hidden>
      <path d="M8 2v12M2 8h12M3.8 3.8l8.4 8.4M12.2 3.8l-8.4 8.4" />
    </svg>
  );
}

/** A neutral speech bubble for ChatGPT (not the brand mark). */
function ChatGptMark() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinejoin="round" aria-hidden>
      <path d="M3.5 3h9A1.5 1.5 0 0 1 14 4.5v5.5a1.5 1.5 0 0 1-1.5 1.5H7.5L4.5 14v-2.5h-1A1.5 1.5 0 0 1 2 10V4.5A1.5 1.5 0 0 1 3.5 3z" />
    </svg>
  );
}
