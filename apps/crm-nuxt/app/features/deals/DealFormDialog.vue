<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import FormDialog from "@/components/crm/FormDialog.vue";
import RecordCombobox from "@/components/crm/RecordCombobox.vue";
import type { PickedRecord } from "@/components/crm/types";
import SelectField from "@/components/crm/SelectField.vue";
import TextField from "@/components/crm/TextField.vue";
import UserSelect from "@/components/crm/UserSelect.vue";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { CompanyRef, Deal, DealInput, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, MAX_AMOUNT_CENTS } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { centsToInput, parseMoneyToCents } from "@/lib/format";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { useCreateDeal, useUpdateDeal } from "./queries";

/**
 * Create or edit a deal. The stage is chosen on create only; afterwards a deal
 * moves with `POST /deals/{id}/move` (the board or the stage bar).
 */
const props = defineProps<{ deal?: Deal; company?: CompanyRef; defaultStage?: DealStage }>();
const open = defineModel<boolean>("open", { default: false });

const CURRENCIES = ["INR", "USD", "EUR", "GBP", "AED", "SGD"] as const;
const picked = z.object({ id: z.string(), label: z.string() });

const schema = z
  .object({
    title: z.string().trim().min(1, "Give the deal a name.").max(255),
    company: picked.nullable().refine((v) => v !== null, "Choose the company this deal is with."),
    contact: picked.nullable(),
    amount: z
      .string()
      .trim()
      .min(1, "Enter the deal value.")
      .refine((v) => {
        const cents = parseMoneyToCents(v);
        return cents !== null && cents >= 0;
      }, "Enter an amount of 0 or more, like 12,50,000."),
    currency: z.string().length(3),
    stage: z.string(),
    expected_close_date: z.string(),
    owner_id: z.string().optional(),
  })
  .superRefine((v, ctx) => {
    const cents = parseMoneyToCents(v.amount, v.currency);
    if (cents !== null && cents > MAX_AMOUNT_CENTS) {
      ctx.addIssue({ code: "custom", path: ["amount"], message: "That amount is larger than the API accepts." });
    }
  });

/** API field names → form field names, for 400 validation errors. */
const FIELD_MAP = {
  title: "title",
  company_id: "company",
  contact_id: "contact",
  amount_cents: "amount",
  currency: "currency",
  stage: "stage",
  expected_close_date: "expected_close_date",
  owner_id: "owner_id",
} as const;

const ability = useAbility<AppAbility>();
const canAssign = computed(() => ability.can("manage", "Deal"));
const create = useCreateDeal();
const update = useUpdateDeal(() => props.deal?.id ?? "");
const rootError = ref<string | null>(null);

function initialValues() {
  const c = props.deal?.company ?? props.company ?? null;
  return {
    title: props.deal?.title ?? "",
    company: c ? { id: c.id, label: c.name } : null,
    contact: props.deal?.contact ? { id: props.deal.contact.id, label: props.deal.contact.full_name } : null,
    amount: props.deal ? centsToInput(props.deal.amount_cents, props.deal.currency) : "",
    currency: props.deal?.currency ?? "INR",
    stage: props.deal?.stage ?? props.defaultStage ?? "lead",
    expected_close_date: props.deal?.expected_close_date ?? "",
    owner_id: props.deal?.owner_id,
  };
}

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: initialValues() });
const { value: company, errorMessage: companyError } = useField<PickedRecord | null>("company");
const { value: contact, errorMessage: contactError } = useField<PickedRecord | null>("contact");
const { value: amount, errorMessage: amountError } = useField<string>("amount");
const { value: currency } = useField<string>("currency");
const { value: ownerId, errorMessage: ownerError } = useField<string | undefined>("owner_id");

watch(open, (isOpen) => {
  if (!isOpen) return;
  form.resetForm({ values: initialValues() });
  rootError.value = null;
});

function pickCompany(next: PickedRecord | null) {
  if (next?.id !== company.value?.id) contact.value = null;
  company.value = next;
}

const stageOptions = DEAL_STAGES.filter((s) => s.stage !== "lost").map((s) => ({
  value: s.stage,
  label: `${s.label} (${s.probability}%)`,
}));

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
  const body: DealInput = {
    title: values.title,
    company_id: values.company!.id,
    contact_id: values.contact?.id ?? null,
    amount_cents: parseMoneyToCents(values.amount, values.currency) ?? 0,
    currency: values.currency,
    expected_close_date: values.expected_close_date || null,
    ...(props.deal ? {} : { stage: values.stage as DealStage }),
    ...(canAssign.value && values.owner_id ? { owner_id: values.owner_id } : {}),
  };
  const onError = (error: unknown) => {
    const root = applyFieldErrors(error, form.setFieldError, FIELD_MAP);
    if (root) rootError.value = root;
  };
  if (props.deal) {
    update.mutate(body, {
      onSuccess: () => {
        toast.success("Deal updated");
        open.value = false;
      },
      onError,
    });
  } else {
    create.mutate(body, {
      onSuccess: (created) => {
        toast.success(`${created.title} created`, {
          action: { label: "Open", onClick: () => void navigateTo(`/deals/${created.id}`) },
        });
        open.value = false;
      },
      onError,
    });
  }
});
</script>

<template>
  <FormDialog
    v-model:open="open"
    :title="deal ? `Edit ${deal.title}` : 'New deal'"
    :description="deal ? undefined : 'Track an opportunity through the pipeline.'"
    :submit-label="deal ? 'Save changes' : 'Create deal'"
    :pending="create.isPending.value || update.isPending.value"
    :root-error="rootError"
    wide
    @submit="onSubmit"
  >
    <template v-if="$slots.trigger" #trigger><slot name="trigger" /></template>
    <TextField name="title" label="Deal name" required autofocus placeholder="Annual platform license" />
    <div class="grid gap-5 sm:grid-cols-2">
      <Field :data-invalid="companyError ? true : undefined">
        <FieldLabel for="deal-company">Company <span class="font-normal text-fg-3">(required)</span></FieldLabel>
        <RecordCombobox
          id="deal-company"
          type="company"
          :model-value="company"
          placeholder="Choose a company"
          :invalid="Boolean(companyError)"
          @update:model-value="pickCompany"
        />
        <FieldError v-if="companyError">{{ companyError }}</FieldError>
      </Field>
      <Field :data-invalid="contactError ? true : undefined">
        <FieldLabel for="deal-contact">Main contact</FieldLabel>
        <RecordCombobox
          id="deal-contact"
          v-model="contact"
          type="contact"
          :company-id="company?.id"
          :placeholder="company ? 'Choose a contact' : 'Choose a company first'"
          :disabled="!company"
          :invalid="Boolean(contactError)"
        />
        <FieldError v-if="contactError">{{ contactError }}</FieldError>
      </Field>
      <Field :data-invalid="amountError ? true : undefined">
        <FieldLabel for="deal-amount">Value <span class="font-normal text-fg-3">(required)</span></FieldLabel>
        <InputGroup>
          <InputGroupInput
            id="deal-amount"
            v-model="amount"
            inputmode="decimal"
            placeholder="12,50,000"
            :aria-invalid="amountError ? true : undefined"
            class="figures"
          />
          <InputGroupAddon align="inline-end" class="pr-1">
            <Select v-model="currency">
              <SelectTrigger size="sm" class="h-7 rounded-md border-0 bg-transparent px-2 text-xs hover:bg-ink-2" aria-label="Currency">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem v-for="c in CURRENCIES" :key="c" :value="c">{{ c }}</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </InputGroupAddon>
        </InputGroup>
        <FieldError v-if="amountError">{{ amountError }}</FieldError>
      </Field>
      <TextField name="expected_close_date" label="Expected close" type="date" />
      <SelectField v-if="!deal" name="stage" label="Stage" :options="stageOptions" />
      <Field v-if="canAssign" :data-invalid="ownerError ? true : undefined">
        <FieldLabel for="deal-owner">Owner</FieldLabel>
        <UserSelect id="deal-owner" v-model="ownerId" :invalid="Boolean(ownerError)" />
        <FieldError v-if="ownerError">{{ ownerError }}</FieldError>
      </Field>
    </div>
  </FormDialog>
</template>
