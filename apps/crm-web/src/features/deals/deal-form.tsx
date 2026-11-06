"use client";

import { useAbility } from "@navree/next/abilities";
import { zodResolver } from "@hookform/resolvers/zod";
import { useRouter } from "next/navigation";
import { useState, type ReactNode } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormDialog } from "@/components/form-dialog";
import { SelectField, TextField } from "@/components/form-fields";
import { RecordCombobox, UserSelect, type PickedRecord } from "@/components/record-pickers";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useCreateIntent } from "@/hooks/use-create-intent";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { CompanyRef, Deal, DealInput, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, MAX_AMOUNT_CENTS } from "@/lib/api/types";
import { centsToInput, parseMoneyToCents } from "@/lib/format";
import { useCreateDeal, useUpdateDeal } from "./queries";

export const CURRENCIES = ["INR", "USD", "EUR", "GBP", "AED", "SGD"] as const;

const picked = z.object({ id: z.string(), label: z.string() });

const schema = z
  .object({
    title: z.string().trim().min(1, "Give the deal a name.").max(255),
    company: picked.nullable(),
    contact: picked.nullable(),
    amount: z.string().trim().min(1, "Enter the deal value."),
    currency: z.string().length(3),
    stage: z.string(),
    expected_close_date: z.string(),
    owner_id: z.string().optional(),
  })
  .superRefine((v, ctx) => {
    if (!v.company)
      ctx.addIssue({ code: "custom", path: ["company"], message: "Choose the company this deal is with." });
    const cents = parseMoneyToCents(v.amount, v.currency);
    if (v.amount && (cents === null || cents < 0)) {
      ctx.addIssue({ code: "custom", path: ["amount"], message: "Enter an amount of 0 or more, like 12,50,000." });
    } else if (cents !== null && cents > MAX_AMOUNT_CENTS) {
      ctx.addIssue({ code: "custom", path: ["amount"], message: "That amount is larger than the API accepts." });
    }
  });
type Values = z.infer<typeof schema>;

function toValues(deal?: Deal, company?: CompanyRef): Values {
  const c = deal?.company ?? company ?? null;
  return {
    title: deal?.title ?? "",
    company: c ? { id: c.id, label: c.name } : null,
    contact: deal?.contact ? { id: deal.contact.id, label: deal.contact.full_name } : null,
    amount: deal ? centsToInput(deal.amount_cents, deal.currency) : "",
    currency: deal?.currency ?? "INR",
    stage: deal?.stage ?? "lead",
    expected_close_date: deal?.expected_close_date ?? "",
    owner_id: deal?.owner_id,
  };
}

/** API field names → form field names, for mapping 400 validation errors. */
const FIELD_MAP: Record<string, keyof Values> = {
  title: "title",
  company_id: "company",
  contact_id: "contact",
  amount_cents: "amount",
  currency: "currency",
  stage: "stage",
  expected_close_date: "expected_close_date",
  owner_id: "owner_id",
};

/**
 * Create or edit a deal. Stage is chosen on create only; afterwards a deal
 * moves through `POST /deals/{id}/move` (board or stage bar).
 */
export function DealFormDialog({
  deal,
  company,
  defaultStage,
  trigger,
  openOnCreateIntent,
}: {
  deal?: Deal;
  company?: CompanyRef;
  defaultStage?: DealStage;
  trigger: ReactNode;
  /** Open when the command menu asks for a new deal (set on one dialog per screen). */
  openOnCreateIntent?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const router = useRouter();
  const ability = useAbility();
  const canAssign = ability.can("manage", "Deal");
  const create = useCreateDeal();
  const update = useUpdateDeal(deal?.id ?? "");
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: toValues(deal, company) });
  const companyValue = useWatch({ control: form.control, name: "company" });

  const onOpenChange = (next: boolean) => {
    if (next) form.reset({ ...toValues(deal, company), ...(defaultStage && !deal ? { stage: defaultStage } : {}) });
    setOpen(next);
  };
  useCreateIntent(openOnCreateIntent && !deal ? "deal" : undefined, () => onOpenChange(true));

  const onSubmit = form.handleSubmit((values) => {
    const body: DealInput = {
      title: values.title,
      company_id: values.company!.id,
      contact_id: values.contact?.id ?? null,
      amount_cents: parseMoneyToCents(values.amount, values.currency) ?? 0,
      currency: values.currency,
      expected_close_date: values.expected_close_date || null,
      ...(deal ? {} : { stage: values.stage as DealStage }),
      ...(canAssign && values.owner_id ? { owner_id: values.owner_id } : {}),
    };
    const onError = (error: unknown) => {
      applyFieldErrors(
        error,
        (name, err) => form.setError(FIELD_MAP[name as string] ?? (name as keyof Values), err),
        Object.keys(FIELD_MAP),
      );
    };
    if (deal) {
      update.mutate(body, {
        onSuccess: () => {
          toast.success("Deal updated");
          setOpen(false);
        },
        onError,
      });
    } else {
      create.mutate(body, {
        onSuccess: (created) => {
          toast.success(`${created.title} created`, {
            action: { label: "Open", onClick: () => router.push(`/deals/${created.id}`) },
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
      title={deal ? `Edit ${deal.title}` : "New deal"}
      description={deal ? undefined : "Track an opportunity through the pipeline."}
      submitLabel={deal ? "Save changes" : "Create deal"}
      pending={create.isPending || update.isPending}
      rootError={form.formState.errors.root?.server?.message}
      onSubmit={onSubmit}
      wide
    >
      <FieldGroup className="gap-4">
        <TextField
          control={form.control}
          name="title"
          label="Deal name"
          required
          autoFocus
          placeholder="Annual platform license"
        />
        <div className="grid gap-4 sm:grid-cols-2">
          <Controller
            control={form.control}
            name="company"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor="deal-company">
                  Company <span className="text-muted-foreground">(required)</span>
                </FieldLabel>
                <RecordCombobox
                  id="deal-company"
                  type="company"
                  value={field.value as PickedRecord | null}
                  onChange={(v) => {
                    field.onChange(v);
                    if (v?.id !== field.value?.id) form.setValue("contact", null);
                  }}
                  placeholder="Choose a company"
                  invalid={fieldState.invalid}
                />
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
          <Controller
            control={form.control}
            name="contact"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor="deal-contact">Main contact</FieldLabel>
                <RecordCombobox
                  id="deal-contact"
                  type="contact"
                  companyId={companyValue?.id}
                  value={field.value as PickedRecord | null}
                  onChange={field.onChange}
                  placeholder={companyValue ? "Choose a contact" : "Choose a company first"}
                  disabled={!companyValue}
                  invalid={fieldState.invalid}
                />
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
          <Controller
            control={form.control}
            name="amount"
            render={({ field, fieldState }) => (
              <Field data-invalid={fieldState.invalid || undefined}>
                <FieldLabel htmlFor="deal-amount">
                  Value <span className="text-muted-foreground">(required)</span>
                </FieldLabel>
                <InputGroup>
                  <InputGroupInput
                    {...field}
                    id="deal-amount"
                    inputMode="decimal"
                    placeholder="12,50,000"
                    aria-invalid={fieldState.invalid || undefined}
                    className="figures"
                  />
                  <InputGroupAddon align="inline-end" className="pr-1">
                    <Controller
                      control={form.control}
                      name="currency"
                      render={({ field: currency }) => (
                        <Select value={currency.value} onValueChange={currency.onChange}>
                          <SelectTrigger size="sm" className="h-6 border-0 px-1.5 shadow-none" aria-label="Currency">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {CURRENCIES.map((c) => (
                                <SelectItem key={c} value={c}>
                                  {c}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      )}
                    />
                  </InputGroupAddon>
                </InputGroup>
                <FieldError errors={[fieldState.error]} />
              </Field>
            )}
          />
          <TextField control={form.control} name="expected_close_date" label="Expected close" type="date" />
          {!deal ? (
            <SelectField
              control={form.control}
              name="stage"
              label="Stage"
              options={DEAL_STAGES.filter((s) => s.stage !== "lost").map((s) => ({
                value: s.stage,
                label: `${s.label} (${s.probability}%)`,
              }))}
            />
          ) : null}
          {canAssign ? (
            <Controller
              control={form.control}
              name="owner_id"
              render={({ field, fieldState }) => (
                <Field data-invalid={fieldState.invalid || undefined}>
                  <FieldLabel htmlFor="deal-owner">Owner</FieldLabel>
                  <UserSelect
                    id="deal-owner"
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
