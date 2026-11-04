"use client";

import { CheckIcon, ChevronsUpDownIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useCompanies } from "@/features/companies/queries";
import { useContacts } from "@/features/contacts/queries";
import { useDeals } from "@/features/deals/queries";
import { useUsers } from "@/features/users/queries";
import { useDebouncedValue } from "@/hooks/use-url-state";
import type { ActivitySubjectType } from "@/lib/api/types";
import { cn } from "@/lib/utils";

export type PickedRecord = { id: string; label: string };

/** Searchable, server-backed record picker (companies, contacts or deals). */
export function RecordCombobox({
  type,
  value,
  onChange,
  placeholder,
  companyId,
  invalid,
  id,
  disabled,
}: {
  type: ActivitySubjectType;
  value: PickedRecord | null;
  onChange: (value: PickedRecord | null) => void;
  placeholder: string;
  /** Limit contacts/deals to one company. */
  companyId?: string;
  invalid?: boolean;
  id?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const term = useDebouncedValue(search, 200) || undefined;

  const companies = useCompanies({ search: term, per_page: 8, sort: "name" }, open && type === "company");
  const contacts = useContacts(
    { search: term, filter: { company_id: companyId }, per_page: 8 },
    open && type === "contact",
  );
  const deals = useDeals({ search: term, filter: { company_id: companyId }, per_page: 8 }, open && type === "deal");

  const options: PickedRecord[] =
    type === "company"
      ? (companies.data?.data.map((c) => ({ id: c.id, label: c.name })) ?? [])
      : type === "contact"
        ? (contacts.data?.data.map((c) => ({ id: c.id, label: c.full_name })) ?? [])
        : (deals.data?.data.map((d) => ({ id: d.id, label: d.title })) ?? []);
  const loading = companies.isFetching || contacts.isFetching || deals.isFetching;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          id={id}
          type="button"
          variant="outline"
          // oxlint-disable-next-line jsx-a11y/role-has-required-aria-props -- Radix sets aria-controls on the trigger
          role="combobox"
          aria-expanded={open}
          aria-invalid={invalid || undefined}
          disabled={disabled}
          className={cn("w-full justify-between font-normal", !value && "text-muted-foreground")}
        >
          <span className="truncate">{value?.label ?? placeholder}</span>
          <ChevronsUpDownIcon data-icon="inline-end" className="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-(--radix-popover-trigger-width) min-w-64 p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput
            placeholder={`Search ${type === "company" ? "companies" : `${type}s`}`}
            value={search}
            onValueChange={setSearch}
          />
          <CommandList>
            <CommandEmpty>{loading ? "Searching…" : "No matches."}</CommandEmpty>
            <CommandGroup>
              {options.map((option) => (
                <CommandItem
                  key={option.id}
                  value={option.id}
                  onSelect={() => {
                    onChange(option.id === value?.id ? null : option);
                    setOpen(false);
                  }}
                >
                  <CheckIcon className={cn(option.id === value?.id ? "opacity-100" : "opacity-0")} />
                  <span className="truncate">{option.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/** Select a teammate (owner or assignee). */
export function UserSelect({
  value,
  onChange,
  id,
  invalid,
  allowEmpty,
  emptyLabel = "Nobody",
}: {
  value: string | undefined;
  onChange: (value: string | undefined) => void;
  id?: string;
  invalid?: boolean;
  allowEmpty?: boolean;
  emptyLabel?: string;
}) {
  const users = useUsers();
  return (
    <Select value={value ?? "__none"} onValueChange={(v) => onChange(v === "__none" ? undefined : v)}>
      <SelectTrigger id={id} className="w-full" aria-invalid={invalid || undefined}>
        <SelectValue placeholder="Choose a teammate" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {allowEmpty ? <SelectItem value="__none">{emptyLabel}</SelectItem> : null}
          {users.data?.data.map((u) => (
            <SelectItem key={u.id} value={u.id}>
              {u.name}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}
