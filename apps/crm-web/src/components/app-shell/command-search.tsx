"use client";

import { Building2Icon, ColumnsIcon, GaugeIcon, ListChecksIcon, SearchIcon, UserIcon, UsersIcon } from "lucide-react";
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
import { Kbd } from "@/components/ui/kbd";
import { useCompanies } from "@/features/companies/queries";
import { useContacts } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import { useDebouncedValue } from "@/hooks/use-url-state";
import { formatMoney } from "@/lib/format";

const PAGES = [
  { href: "/", label: "Dashboard", icon: GaugeIcon },
  { href: "/companies", label: "Companies", icon: Building2Icon },
  { href: "/contacts", label: "Contacts", icon: UsersIcon },
  { href: "/deals", label: "Deals", icon: ColumnsIcon },
  { href: "/tasks", label: "Tasks", icon: ListChecksIcon },
];

/** Global search across companies, contacts and deals (Ctrl/⌘ K). */
export function CommandSearch() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const router = useRouter();
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
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const go = (href: string) => {
    setOpen(false);
    setQuery("");
    router.push(href);
  };

  // Results arrive asynchronously, so keep the highlighted item in state and default it to the first result.
  const [highlighted, setHighlighted] = useState("");
  const firstValue =
    term.length < 2
      ? PAGES[0].label
      : companies.data?.data[0]
        ? `company-${companies.data.data[0].id}`
        : contacts.data?.data[0]
          ? `contact-${contacts.data.data[0].id}`
          : deals.data?.data[0]
            ? `deal-${deals.data.data[0].id}`
            : "";

  const searching = enabled && (companies.isFetching || contacts.isFetching || deals.isFetching);
  const results = enabled
    ? (companies.data?.data.length ?? 0) + (contacts.data?.data.length ?? 0) + (deals.data?.data.length ?? 0)
    : 0;

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex h-8 w-full max-w-sm items-center gap-2 rounded-lg border border-input bg-card px-2.5 text-sm text-muted-foreground transition-colors hover:border-ring/40 hover:text-foreground"
      >
        <SearchIcon className="size-4" />
        <span className="flex-1 truncate text-left">Search companies, contacts, deals</span>
        <Kbd className="hidden sm:inline-flex">Ctrl K</Kbd>
      </button>
      <CommandDialog
        open={open}
        onOpenChange={setOpen}
        title="Search"
        description="Search companies, contacts and deals"
      >
        {/* Results come from the API, so cmdk's own filtering is off. */}
        <Command shouldFilter={false} value={highlighted || firstValue} onValueChange={setHighlighted}>
          <CommandInput
            placeholder="Type a name, email or deal title"
            value={query}
            onValueChange={(q) => {
              setQuery(q);
              setHighlighted("");
            }}
          />
          <CommandList>
            {term.length < 2 ? (
              <CommandGroup heading="Go to">
                {PAGES.map(({ href, label, icon: Icon }) => (
                  <CommandItem key={href} value={label} onSelect={() => go(href)}>
                    <Icon />
                    {label}
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : (
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
                        <StageDot stage={d.stage} />
                        <span className="truncate">{d.title}</span>
                        <CommandShortcut className="figures">
                          {formatMoney(d.amount_cents, d.currency, { compact: true })}
                        </CommandShortcut>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ) : null}
              </>
            )}
          </CommandList>
        </Command>
      </CommandDialog>
    </>
  );
}
