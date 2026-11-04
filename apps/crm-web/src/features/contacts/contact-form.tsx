"use client";

import { useAbility } from "@navree/next/abilities";
import { zodResolver } from "@hookform/resolvers/zod";
import { useRouter } from "next/navigation";
import { useState, type ReactNode } from "react";
import { Controller, useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormDialog } from "@/components/form-dialog";
import { nullable, SelectField, TextField } from "@/components/form-fields";
import { RecordCombobox, UserSelect, type PickedRecord } from "@/components/record-pickers";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { useCreateIntent } from "@/hooks/use-create-intent";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { CompanyRef, Contact, ContactInput, ContactStatus, ContactUpdateInput } from "@/lib/api/types";
import { CONTACT_STATUSES } from "@/lib/api/types";
import { useCreateContact, useUpdateContact } from "./queries";

export const STATUS_LABEL: Record<ContactStatus, string> = {
  lead: "Lead",
  prospect: "Prospect",
  customer: "Customer",
  churned: "Churned",
};

const schema = z.object({
  first_name: z.string().trim().min(1, "Enter a first name.").max(100),
  last_name: z.string().trim().min(1, "Enter a last name.").max(100),
  email: z.union([z.literal(""), z.email("Enter a valid email address.")]),
  phone: z.string().trim().max(50),
  title: z.string().trim().max(150),
  status: z.enum(CONTACT_STATUSES as unknown as [ContactStatus, ...ContactStatus[]]),
  company: z.object({ id: z.string(), label: z.string() }).nullable(),
  owner_id: z.string().optional(),
});
type Values = z.infer<typeof schema>;

const FIELDS = ["first_name", "last_name", "email", "phone", "title", "status", "company_id", "owner_id"];

function toValues(contact?: Contact, company?: CompanyRef | null): Values {
  const c = contact?.company ?? company ?? null;
  return {
    first_name: contact?.first_name ?? "",
    last_name: contact?.last_name ?? "",
    email: contact?.email ?? "",
    phone: contact?.phone ?? "",
    title: contact?.title ?? "",
    status: contact?.status ?? "lead",
    company: c ? { id: c.id, label: c.name } : null,
    owner_id: contact?.owner_id,
  };
}

export function ContactFormDialog({
  contact,
  company,
  trigger,
  openOnCreateIntent,
}: {
  contact?: Contact;
  /** Pre-selects the company when adding from a company page. */
  company?: CompanyRef;
  trigger: ReactNode;
  /** Open when the command menu asks for a new contact (set on one dialog per screen). */
  openOnCreateIntent?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const router = useRouter();
  const ability = useAbility();
  const canAssign = ability.can("manage", "Contact");
  const create = useCreateContact();
  const update = useUpdateContact(contact?.id ?? "");
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: toValues(contact, company) });

  const onOpenChange = (next: boolean) => {
    if (next) form.reset(toValues(contact, company));
    setOpen(next);
  };
  useCreateIntent(openOnCreateIntent && !contact ? "contact" : undefined, () => onOpenChange(true));

  const onSubmit = form.handleSubmit((values) => {
    // Valid for both POST and PATCH (PATCH takes no null owner_id).
    const body: ContactInput & ContactUpdateInput = {
      first_name: values.first_name,
      last_name: values.last_name,
      email: nullable(values.email),
      phone: nullable(values.phone),
      title: nullable(values.title),
      status: values.status,
      company_id: values.company?.id ?? null,
      ...(canAssign && values.owner_id ? { owner_id: values.owner_id } : {}),
    };
    const onError = (error: unknown) => {
      // The API reports the company as company_id; show it on the picker.
      applyFieldErrors(
        error,
        (name, err) => form.setError(name === "company_id" ? "company" : (name as keyof Values), err),
        [...FIELDS],
      );
    };
    if (contact) {
      update.mutate(body, {
        onSuccess: () => {
          toast.success("Contact updated");
          setOpen(false);
        },
        onError,
      });
    } else {
      create.mutate(body, {
        onSuccess: (created) => {
          toast.success(`${created.full_name} added`, {
            action: { label: "Open", onClick: () => router.push(`/contacts/${created.id}`) },
          });
          setOpen(false);
        },
        onError,
      });
    }
  });

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      trigger={trigger}
      title={contact ? `Edit ${contact.full_name}` : "New contact"}
      description={contact ? undefined : "Add a person you're talking to."}
      submitLabel={contact ? "Save changes" : "Create contact"}
      pending={create.isPending || update.isPending}
      rootError={form.formState.errors.root?.server?.message}
      onSubmit={onSubmit}
      wide
    >
      <FieldGroup className="gap-4">
        <div className="grid gap-4 sm:grid-cols-2">
          <TextField control={form.control} name="first_name" label="First name" required autoFocus />
          <TextField control={form.control} name="last_name" label="Last name" required />
          <TextField control={form.control} name="email" label="Email" type="email" />
          <TextField control={form.control} name="phone" label="Phone" type="tel" />
          <TextField control={form.control} name="title" label="Job title" />
          <SelectField
            control={form.control}
            name="status"
            label="Status"
            options={CONTACT_STATUSES.map((s) => ({ value: s, label: STATUS_LABEL[s] }))}
          />
          <Controller
            control={form.control}
            name="company"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor="contact-company">Company</FieldLabel>
                <RecordCombobox
                  id="contact-company"
                  type="company"
                  value={field.value as PickedRecord | null}
                  onChange={field.onChange}
                  placeholder="No company"
                  invalid={fieldState.invalid}
                />
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
          {canAssign ? (
            <Controller
              control={form.control}
              name="owner_id"
              render={({ field, fieldState }) => (
                <Field data-invalid={fieldState.invalid || undefined}>
                  <FieldLabel htmlFor="contact-owner">Owner</FieldLabel>
                  <UserSelect
                    id="contact-owner"
                    value={field.value}
                    onChange={field.onChange}
                    invalid={fieldState.invalid}
                  />
                  <FieldError errors={[fieldState.error]} />
                </Field>
              )}
            />
          ) : null}
        </div>
      </FieldGroup>
    </FormDialog>
  );
}
