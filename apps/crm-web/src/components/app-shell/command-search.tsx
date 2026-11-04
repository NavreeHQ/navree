"use client";

import { useAbility } from "@navree/next/abilities";
import {
  Building2Icon,
  CornerDownLeftIcon,
  GaugeIcon,
  KanbanIcon,
  ListChecksIcon,
  LogOutIcon,
  PlusIcon,
  ScrollTextIcon,
  SearchIcon,
  SettingsIcon,
  UserCogIcon,
  UserIcon,
  UsersIcon,
  type LucideIcon,
} from "lucide-react";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { StageDot } from "@/components/stage";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { useCurrentUser, useLogout } from "@/features/auth/queries";
import { useCompanies } from "@/features/companies/queries";
import { useContacts } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import { requestCreate, type CreateKind } from "@/hooks/use-create-intent";
import { useDebouncedValue } from "@/hooks/use-url-state";
import type { SubjectName } from "@/lib/api/types";
import { formatMoney } from "@/lib/format";

type Page = { href: string; label: string; icon: LucideIcon; keywords?: string[] };

const PAGES: Page[] = [
  { href: "/", label: "Dashboard", icon: GaugeIcon, keywords: ["home", "overview"] },
  { href: "/companies", label: "Companies", icon: Building2Icon, keywords: ["accounts"] },
  { href: "/contacts", label: "Contacts", icon: UsersIcon, keywords: ["people"] },
  { href: "/deals", label: "Deals", icon: KanbanIcon, keywords: ["pipeline", "board"] },
  { href: "/tasks", label: "Tasks", icon: ListChecksIcon, keywords: ["follow-ups", "todo"] },
  { href: "/settings", label: "Settings", icon: SettingsIcon, keywords: ["profile", "permissions"] },
];

const CREATE: { kind: CreateKind; href: string; label: string; subject: SubjectName }[] = [
  { kind: "deal", href: "/deals", label: "New deal", subject: "Deal" },
  { kind: "company", href: "/companies", label: "New company", subject: "Company" },
  { kind: "contact", href: "/contacts", label: "New contact", subject: "Contact" },
  { kind: "task", href: "/tasks", label: "New task", subject: "Activity" },
];

const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const OPEN_EVENT = "navree:command-menu";

/** Opens the command menu from elsewhere in the shell (the workspace menu). */
export function openCommandMenu(): void {
  window.dispatchEvent(new Event(OPEN_EVENT));
}

/** Whether a static command matches what was typed (its label or a keyword). */
function matches(filter: string, label: string, keywords: readonly string[] = []): boolean {
  return !filter || [label, ...keywords].some((k) => k.toLowerCase().includes(filter));
}

/**
 * The command menu (Ctrl/Cmd K): jump to a screen, create a record, sign out,
 * or search companies, contacts and deals through the API.
 */
export function CommandSearch() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  // Rendered only in the browser (behind sign-in), so the platform is known on first render.
  const [mod] = useState(() => (isMac() ? "⌘" : "Ctrl"));
  const router = useRouter();
  const ability = useAbility();
  const me = useCurrentUser();
  const logout = useLogout();
  const term = useDebouncedValue(query.trim(), 200);
  const enabled = open && term.length >= 2;

  const companies = useCompanies({ search: term, per_page: 5 }, enabled);
  const contacts = useContacts({ search: term, per_page: 5 }, enabled);
  const deals = useDeals({ search: term, per_page: 5 }, enabled);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onOpen = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_EVENT, onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_EVENT, onOpen);
    };
  }, []);

  const close = () => {
    setOpen(false);
    setQuery("");
  };
  const go = (href: string) => {
    close();
    router.push(href);
  };
  const create = (kind: CreateKind, href: string) => {
    close();
    router.push(href);
    requestCreate(kind);
  };
  const signOut = () => {
    close();
    logout.mutate(
      {},
      {
        onSettled: () => {
          ability.update([]);
          router.replace("/login");
        },
      },
    );
  };

  const pages: Page[] = [
    ...PAGES,
    ...(ability.can("create", "User")
      ? [{ href: "/settings/users", label: "Users and roles", icon: UserCogIcon, keywords: ["team", "roles"] }]
      : []),
    ...(me.data?.roles.includes("admin")
      ? [{ href: "/settings/audit", label: "Audit log", icon: ScrollTextIcon, keywords: ["history"] }]
      : []),
  ];
  const creatable = CREATE.filter((c) => ability.can("create", c.subject));
  const filter = query.trim().toLowerCase();
  const shownPages = pages.filter((p) => matches(filter, p.label, p.keywords));
  const shownCreate = creatable.filter((c) => matches(filter, c.label, ["add", "create", c.kind]));
  const showSignOut = matches(filter, "Sign out", ["log out", "logout"]);
  const hasCommands = shownPages.length + shownCreate.length > 0 || showSignOut;

  // Results arrive asynchronously, so keep the highlighted item in state and default it to the first command or result.
  const [highlighted, setHighlighted] = useState("");
  const firstResult = !enabled
    ? ""
    : companies.data?.data[0]
      ? `company-${companies.data.data[0].id}`
      : contacts.data?.data[0]
        ? `contact-${contacts.data.data[0].id}`
        : deals.data?.data[0]
          ? `deal-${deals.data.data[0].id}`
          : "";
  const firstValue = shownPages[0]?.label ?? shownCreate[0]?.label ?? (showSignOut ? "Sign out" : firstResult);

  const searching = enabled && (companies.isFetching || contacts.isFetching || deals.isFetching);
  const results = enabled
    ? (companies.data?.data.length ?? 0) + (contacts.data?.data.length ?? 0) + (deals.data?.data.length ?? 0)
    : 0;

  return (
    <>
      {/* An icon button on phones; a search field with its shortcut from md up. */}
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-keyshortcuts="Control+K Meta+K"
        className="flex size-8 shrink-0 items-center justify-center gap-2 rounded-md text-sm text-fg-2 transition-colors outline-none hover:bg-ink-2 hover:text-fg focus-visible:ring-2 focus-visible:ring-ring/60 md:w-56 md:justify-start md:border md:border-line-2 md:bg-ink-1 md:pr-1 md:pl-2.5 md:text-fg-3 md:hover:border-fg-3/40 md:hover:bg-ink-1 md:hover:text-fg-2 lg:w-72"
      >
        <SearchIcon className="size-4 shrink-0" aria-hidden="true" />
        <span className="sr-only flex-1 truncate text-left md:not-sr-only">Search…</span>
        <KbdGroup className="hidden md:inline-flex">
          <Kbd>{mod}</Kbd>
          <Kbd>K</Kbd>
        </KbdGroup>
      </button>
      <CommandDialog
        open={open}
        onOpenChange={(next) => (next ? setOpen(true) : close())}
        title="Command menu"
        description="Jump to a screen, create a record, or search companies, contacts and deals"
      >
        {/* Search results come from the API, so cmdk's own filtering only runs on the static commands. */}
        <Command shouldFilter={false} value={highlighted || firstValue} onValueChange={setHighlighted}>
          <CommandInput
            placeholder="Search records or type a command"
            value={query}
            onValueChange={(q) => {
              setQuery(q);
              setHighlighted("");
            }}
          />
          <CommandList>
            {term.length < 2 || hasCommands ? (
              <StaticCommands
                pages={shownPages}
                creatable={shownCreate}
                showSignOut={showSignOut}
                onGo={go}
                onCreate={create}
                onSignOut={signOut}
              />
            ) : null}
            {term.length >= 2 ? (
              <>
                <CommandEmpty>{searching ? "Searching…" : `Nothing matches “${term}”.`}</CommandEmpty>
                {results > 0 && companies.data?.data.length ? (
                  <CommandGroup heading="Companies">
                    {companies.data.data.map((c) => (
                      <CommandItem key={c.id} value={`company-${c.id}`} onSelect={() => go(`/companies/${c.id}`)}>
                        <Building2Icon />
                        <span className="truncate">{c.name}</span>
                        <CommandShortcut>{c.domain}</CommandShortcut>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null}
                {results > 0 && contacts.data?.data.length ? (
                  <CommandGroup heading="Contacts">
                    {contacts.data.data.map((c) => (
                      <CommandItem key={c.id} value={`contact-${c.id}`} onSelect={() => go(`/contacts/${c.id}`)}>
                        <UserIcon />
                        <span className="truncate">{c.full_name}</span>
                        <CommandShortcut>{c.company?.name}</CommandShortcut>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null}
                {results > 0 && deals.data?.data.length ? (
                  <CommandGroup heading="Deals">
                    {deals.data.data.map((d) => (
                      <CommandItem key={d.id} value={`deal-${d.id}`} onSelect={() => go(`/deals/${d.id}`)}>
                        <StageDot stage={d.stage} className="mx-1" />
                        <span className="truncate">{d.title}</span>
                        <CommandShortcut className="figures">
                          {formatMoney(d.amount_cents, d.currency, { compact: true })}
                        </CommandShortcut>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null}
              </>
            ) : null}
          </CommandList>
          <div
            className="hidden items-center gap-4 border-t bg-ink-0/40 px-4 py-2 text-xs text-fg-3 sm:flex"
            aria-hidden="true"
          >
            <span className="flex items-center gap-1.5">
              <Kbd>↑</Kbd>
              <Kbd>↓</Kbd>
              to move
            </span>
            <span className="flex items-center gap-1.5">
              <Kbd>
                <CornerDownLeftIcon />
              </Kbd>
              to open
            </span>
            <span className="ml-auto flex items-center gap-1.5">
              <Kbd>Esc</Kbd>
              to close
            </span>
          </div>
        </Command>
      </CommandDialog>
    </>
  );
}

/** The commands (screens, new records, signing out), already filtered by what was typed; they stay above a search's results. */
function StaticCommands({
  pages: shownPages,
  creatable: shownCreate,
  showSignOut,
  onGo,
  onCreate,
  onSignOut,
}: {
  pages: Page[];
  creatable: typeof CREATE;
  showSignOut: boolean;
  onGo: (href: string) => void;
  onCreate: (kind: CreateKind, href: string) => void;
  onSignOut: () => void;
}) {
  if (shownPages.length + shownCreate.length === 0 && !showSignOut) {
    return <p className="py-10 text-center text-sm text-fg-2">Keep typing to search records.</p>;
  }
  return (
    <>
      {shownPages.length ? (
        <CommandGroup heading="Go to">
          {shownPages.map(({ href, label, icon: Icon }) => (
            <CommandItem key={href} value={label} onSelect={() => onGo(href)}>
              <Icon />
              {label}
            </CommandItem>
          ))}
        </CommandGroup>
      ) : null}
      {shownCreate.length ? (
        <CommandGroup heading="Create">
          {shownCreate.map((c) => (
            <CommandItem key={c.kind} value={c.label} onSelect={() => onCreate(c.kind, c.href)}>
              <PlusIcon />
              {c.label}
            </CommandItem>
          ))}
        </CommandGroup>
      ) : null}
      {showSignOut ? (
        <CommandGroup heading="Account">
          <CommandItem value="Sign out" onSelect={onSignOut}>
            <LogOutIcon />
            Sign out
          </CommandItem>
        </CommandGroup>
      ) : null}
    </>
  );
}
