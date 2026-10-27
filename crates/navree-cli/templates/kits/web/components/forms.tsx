"use client";

import type { ComponentProps } from "react";
import { type Control, Controller, type FieldPath, type FieldValues } from "react-hook-form";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

interface TextFieldProps<T extends FieldValues> {
  control: Control<T>;
  name: FieldPath<T>;
  label: string;
  description?: string;
  type?: ComponentProps<typeof Input>["type"];
  autoComplete?: string;
  autoFocus?: boolean;
  multiline?: boolean;
}

/** A labelled input bound to a react-hook-form field, with its error (the API's ones too). */
export function TextField<T extends FieldValues>({
  control,
  name,
  label,
  description,
  type = "text",
  autoComplete,
  autoFocus,
  multiline,
}: TextFieldProps<T>) {
  return (
    <Controller
      control={control}
      name={name}
      render={({ field, fieldState }) => (
        <Field data-invalid={fieldState.invalid || undefined}>
          <FieldLabel htmlFor={name}>{label}</FieldLabel>
          {multiline ? (
            <Textarea
              {...field}
              value={field.value ?? ""}
              id={name}
              rows={4}
              aria-invalid={fieldState.invalid || undefined}
            />
          ) : (
            <Input
              {...field}
              value={field.value ?? ""}
              id={name}
              type={type}
              autoComplete={autoComplete}
              autoFocus={autoFocus}
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

/** A form-level message: an error (`variant="destructive"`) or a notice. */
export function FormAlert({ message, variant = "destructive" }: { message?: string; variant?: "default" | "destructive" }) {
  if (!message) return null;
  return (
    <Alert variant={variant} role={variant === "destructive" ? "alert" : "status"}>
      <AlertDescription>{message}</AlertDescription>
    </Alert>
  );
}
