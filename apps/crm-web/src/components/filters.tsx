"use client";

import { SearchIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUsers } from "@/features/users/queries";
import { cn } from "@/lib/utils";

/**
 * Search box bound to a URL param: applies 250ms after typing stops, or at once
 * on Enter/blur. A pending update is dropped if the page unmounts, and the box
 * follows the URL when something else changes it (e.g. "Clear filters").
 */
export function SearchFilter({
  value,
  onChange,
  placeholder,
  className,
}: {
  value: string | undefined;
  onChange: (value: string) => void;
  placeholder: string;
  className?: string;
}) {
  const current = value ?? "";
  const [draft, setDraft] = useState(current);
  const [synced, setSynced] = useState(current);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // The URL changed without us (cleared filters, back button): show it.
  if (current !== synced) {
    setSynced(current);
    setDraft(current);
  }

  useEffect(() => () => clearTimeout(timer.current), []);

  const commit = (next: string) => {
    clearTimeout(timer.current);
    if (next === current) return;
    setSynced(next);
    onChange(next);
  };

  return (
    <InputGroup className={cn("h-8 w-full bg-card sm:w-64", className)}>
      <InputGroupAddon>
        <SearchIcon />
      </InputGroupAddon>
      <InputGroupInput
        type="search"
        aria-label={placeholder}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => {
          const next = e.target.value;
          setDraft(next);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => commit(next), 250);
        }}
        onBlur={() => commit(draft)}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit(draft);
        }}
      />
      {draft ? (
        <InputGroupAddon align="inline-end">
          <InputGroupButton
            size="icon-xs"
            aria-label="Clear search"
            onClick={() => {
              setDraft("");
              commit("");
            }}
          >
            <XIcon />
          </InputGroupButton>
        </InputGroupAddon>
      ) : null}
    </InputGroup>
  );
}

const ALL = "__all";

/** Filter by owner; "Mine" is listed first. */
export function OwnerFilter({
  value,
  onChange,
  currentUserId,
  label = "Owner",
}: {
  value: string | undefined;
  onChange: (value: string | undefined) => void;
  currentUserId?: string;
  label?: string;
}) {
  const users = useUsers();
  return (
    <Select value={value ?? ALL} onValueChange={(v) => onChange(v === ALL ? undefined : v)}>
      <SelectTrigger className="h-8 min-w-36 bg-card" aria-label={`Filter by ${label.toLowerCase()}`}>
        <SelectValue placeholder={`Any ${label.toLowerCase()}`} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem value={ALL}>Any {label.toLowerCase()}</SelectItem>
          {currentUserId ? <SelectItem value={currentUserId}>Mine</SelectItem> : null}
        </SelectGroup>
        <SelectSeparator />
        <SelectGroup>
          {users.data?.data
            .filter((u) => u.id !== currentUserId)
            .map((u) => (
              <SelectItem key={u.id} value={u.id}>
                {u.name}
              </SelectItem>
            ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}

/** A select over a fixed list of options with an "any" choice. */
export function OptionFilter<T extends string>({
  value,
  onChange,
  options,
  anyLabel,
  ariaLabel,
  className,
}: {
  value: T | undefined;
  onChange: (value: T | undefined) => void;
  options: readonly { value: T; label: string }[];
  anyLabel: string;
  ariaLabel: string;
  className?: string;
}) {
  return (
    <Select value={value ?? ALL} onValueChange={(v) => onChange(v === ALL ? undefined : (v as T))}>
      <SelectTrigger className={cn("h-8 min-w-32 bg-card", className)} aria-label={ariaLabel}>
        <SelectValue placeholder={anyLabel} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectItem value={ALL}>{anyLabel}</SelectItem>
          {options.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}
