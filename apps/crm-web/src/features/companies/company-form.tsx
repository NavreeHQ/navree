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
import { UserSelect } from "@/components/record-pickers";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { useCreateIntent } from "@/hooks/use-create-intent";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { Company, CompanyInput, CompanySize, CompanyUpdateInput } from "@/lib/api/types";
import { COMPANY_SIZES } from "@/lib/api/types";
import { useCreateCompany, useUpdateCompany } from "./queries";

export const INDUSTRIES = [
  "Aerospace",
  "Agriculture",
  "Biotech",
  "Design",
  "Education",
  "Energy",
  "Financial services",
  "Food & beverage",
  "Healthcare",
  "Hospitality",
  "Legal",
  "Logistics",
  "Manufacturing",
  "Media",
  "Real estate",
  "Retail",
  "Software",
] as const;

const schema = z.object({
  name: z.string().trim().min(1, "Enter the company name.").max(255),
  domain: z
    .string()
    .trim()
    .refine((v) => v === "" || /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(v), "Use a bare domain, like agra.com."),
  website: z
    .string()
    .trim()
    .refine((v) => v === "" || /^https?:\/\/\S+\.\S+/.test(v), "Use a full address starting with https://"),
  industry: z.string().trim().max(100),
  size: z.string(),
  phone: z.string().trim().max(50),
  city: z.string().trim().max(100),
  country: z.string().trim().max(100),
  owner_id: z.string().optional(),
});
type Values = z.infer<typeof schema>;

const FIELDS = ["name", "domain", "website", "industry", "size", "phone", "city", "country", "owner_id"];

function toValues(company?: Company): Values {
  return {
    name: company?.name ?? "",
    domain: company?.domain ?? "",
    website: company?.website ?? "",
    industry: company?.industry ?? "",
    size: company?.size ?? "",
    phone: company?.phone ?? "",
    city: company?.city ?? "",
    country: company?.country ?? "",
    owner_id: company?.owner_id,
  };
}

/** Create or edit a company. Owner can only be chosen by people who manage companies. */
export function CompanyFormDialog({
  company,
  trigger,
  openOnCreateIntent,
}: {
  company?: Company;
  trigger: ReactNode;
  /** Open when the command menu asks for a new company (set on one dialog per screen). */
  openOnCreateIntent?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const router = useRouter();
  const ability = useAbility();
  const canAssign = ability.can("manage", "Company");
  const create = useCreateCompany();
  const update = useUpdateCompany(company?.id ?? "");
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: toValues(company) });

  const onOpenChange = (next: boolean) => {
    if (next) form.reset(toValues(company));
    setOpen(next);
  };
  useCreateIntent(openOnCreateIntent && !company ? "company" : undefined, () => onOpenChange(true));

  const onSubmit = form.handleSubmit((values) => {
    // Valid for both POST and PATCH (PATCH takes no null owner_id).
    const body: CompanyInput & CompanyUpdateInput = {
      name: values.name,
      domain: nullable(values.domain)?.toLowerCase() ?? null,
      website: nullable(values.website),
      industry: nullable(values.industry),
      size: (nullable(values.size) as CompanySize | null) ?? null,
      phone: nullable(values.phone),
      city: nullable(values.city),
      country: nullable(values.country),
      ...(canAssign && values.owner_id ? { owner_id: values.owner_id } : {}),
    };
    const onError = (error: unknown) => applyFieldErrors(error, form.setError, FIELDS);
    if (company) {
      update.mutate(body, {
        onSuccess: () => {
          toast.success("Company updated");
          setOpen(false);
        },
        onError,
      });
    } else {
      create.mutate(body, {
        onSuccess: (created) => {
          toast.success(`${created.name} added`, {
            action: { label: "Open", onClick: () => router.push(`/companies/${created.id}`) },
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
      title={company ? `Edit ${company.name}` : "New company"}
      description={company ? undefined : "Add an account your team is selling to."}
      submitLabel={company ? "Save changes" : "Create company"}
      pending={create.isPending || update.isPending}
      rootError={form.formState.errors.root?.server?.message}
      onSubmit={onSubmit}
      wide
    >
      <FieldGroup className="gap-4">
        <TextField control={form.control} name="name" label="Name" required autoFocus />
        <div className="grid gap-4 sm:grid-cols-2">
          <TextField control={form.control} name="domain" label="Domain" placeholder="agra.com" />
          <TextField control={form.control} name="website" label="Website" placeholder="https://agra.com" type="url" />
          <TextField control={form.control} name="industry" label="Industry" list="industry-options" />
          <SelectField
            control={form.control}
            name="size"
            label="Employees"
            noneLabel="Unknown"
            options={COMPANY_SIZES.map((s) => ({ value: s, label: s }))}
          />
          <TextField control={form.control} name="phone" label="Phone" type="tel" />
          <TextField control={form.control} name="city" label="City" />
          <TextField control={form.control} name="country" label="Country" />
          {canAssign ? (
            <Controller
              control={form.control}
              name="owner_id"
              render={({ field, fieldState }) => (
                <Field data-invalid={fieldState.invalid || undefined}>
                  <FieldLabel htmlFor="company-owner">Owner</FieldLabel>
                  <UserSelect
                    id="company-owner"
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
        <datalist id="industry-options">
          {INDUSTRIES.map((i) => (
            <option key={i} value={i} />
          ))}
        </datalist>
      </FieldGroup>
    </FormDialog>
  );
}
