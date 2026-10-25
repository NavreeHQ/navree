"use client";

import type { ReactNode } from "react";
import { Controller, type Control, type FieldPath, type FieldValues } from "react-hook-form";
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

/** A labelled text input bound to react-hook-form, with server/client errors. */
export function TextField<T extends FieldValues>({
  control,
  name,
  label,
  type = "text",
  placeholder,
  description,
  autoFocus,
  autoComplete,
  multiline,
  required,
  inputMode,
  list,
}: {
  control: Control<T>;
  name: FieldPath<T>;
  label: string;
  type?: string;
  placeholder?: string;
  description?: ReactNode;
  autoFocus?: boolean;
  autoComplete?: string;
  multiline?: boolean;
  required?: boolean;
  inputMode?: React.HTMLAttributes<HTMLInputElement>["inputMode"];
  list?: string;
}) {
  const id = `field-${String(name)}`;
  return (
    <Controller
      control={control}
      name={name}
      render={({ field, fieldState }) => (
        <Field data-invalid={fieldState.invalid || undefined}>
          <FieldLabel htmlFor={id}>
            {label}
            {required ? <span className="text-muted-foreground">(required)</span> : null}
          </FieldLabel>
          {multiline ? (
            <Textarea
              {...field}
              value={field.value ?? ""}
              id={id}
              placeholder={placeholder}
              aria-invalid={fieldState.invalid || undefined}
            />
          ) : (
            <Input
              {...field}
              value={field.value ?? ""}
              id={id}
              type={type}
              placeholder={placeholder}
              autoFocus={autoFocus}
              autoComplete={autoComplete}
              inputMode={inputMode}
              list={list}
              aria-invalid={fieldState.invalid || undefined}
            />
          )}
          {description ? <FieldDescription>{description}</FieldDescription> : null}
          <FieldError errors={[fieldState.error]} />
        </Field>
      )}
    />
  );
}

/** A labelled select bound to react-hook-form. Empty string means "none". */
export function SelectField<T extends FieldValues>({
  control,
  name,
  label,
  options,
  placeholder,
  noneLabel,
}: {
  control: Control<T>;
  name: FieldPath<T>;
  label: string;
  options: readonly { value: string; label: string }[];
  placeholder?: string;
  /** When set, offers an explicit "none" choice (stored as ""). */
  noneLabel?: string;
}) {
  const id = `field-${String(name)}`;
  return (
    <Controller
      control={control}
      name={name}
      render={({ field, fieldState }) => (
        <Field data-invalid={fieldState.invalid || undefined}>
          <FieldLabel htmlFor={id}>{label}</FieldLabel>
          <Select
            value={field.value ? String(field.value) : noneLabel ? "__none" : undefined}
            onValueChange={(v) => field.onChange(v === "__none" ? "" : v)}
          >
            <SelectTrigger id={id} className="w-full" aria-invalid={fieldState.invalid || undefined}>
              <SelectValue placeholder={placeholder} />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {noneLabel ? <SelectItem value="__none">{noneLabel}</SelectItem> : null}
                {options.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <FieldError errors={[fieldState.error]} />
        </Field>
      )}
    />
  );
}

/** Converts "" to null for optional API fields. */
export function nullable(value: string | undefined | null): string | null {
  const v = (value ?? "").trim();
  return v === "" ? null : v;
}
