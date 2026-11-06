<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import FormDialog from "@/components/crm/FormDialog.vue";
import SelectField from "@/components/crm/SelectField.vue";
import TextField from "@/components/crm/TextField.vue";
import UserSelect from "@/components/crm/UserSelect.vue";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { Company, CompanyInput, CompanySize, CompanyUpdateInput } from "@/lib/api/types";
import { COMPANY_SIZES } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { nullable } from "@/lib/forms/values";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { INDUSTRIES } from "./industries";
import { useCreateCompany, useUpdateCompany } from "./queries";

/** Create or edit a company. Only people who manage companies choose the owner. */
const props = defineProps<{ company?: Company }>();
const open = defineModel<boolean>("open", { default: false });

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

const FIELDS = ["name", "domain", "website", "industry", "size", "phone", "city", "country", "owner_id"] as const;

const ability = useAbility<AppAbility>();
const canAssign = computed(() => ability.can("manage", "Company"));
const create = useCreateCompany();
const update = useUpdateCompany(() => props.company?.id ?? "");
const rootError = ref<string | null>(null);

function initialValues() {
  const c = props.company;
  return {
    name: c?.name ?? "",
    domain: c?.domain ?? "",
    website: c?.website ?? "",
    industry: c?.industry ?? "",
    size: c?.size ?? "",
    phone: c?.phone ?? "",
    city: c?.city ?? "",
    country: c?.country ?? "",
    owner_id: c?.owner_id,
  };
}

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: initialValues() });
const { value: ownerId, errorMessage: ownerError } = useField<string | undefined>("owner_id");

watch(open, (isOpen) => {
  if (!isOpen) return;
  form.resetForm({ values: initialValues() });
  rootError.value = null;
});

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
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
    ...(canAssign.value && values.owner_id ? { owner_id: values.owner_id } : {}),
  };
  const onError = (error: unknown) => {
    const root = applyFieldErrors(error, form.setFieldError, FIELDS);
    if (root) rootError.value = root;
  };
  if (props.company) {
    update.mutate(body, {
      onSuccess: () => {
        toast.success("Company updated");
        open.value = false;
      },
      onError,
    });
  } else {
    create.mutate(body, {
      onSuccess: (created) => {
        toast.success(`${created.name} added`, {
          action: { label: "Open", onClick: () => void navigateTo(`/companies/${created.id}`) },
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
    :title="company ? `Edit ${company.name}` : 'New company'"
    :description="company ? undefined : 'Add an account your team is selling to.'"
    :submit-label="company ? 'Save changes' : 'Create company'"
    :pending="create.isPending.value || update.isPending.value"
    :root-error="rootError"
    wide
    @submit="onSubmit"
  >
    <template v-if="$slots.trigger" #trigger><slot name="trigger" /></template>
    <TextField name="name" label="Name" required autofocus />
    <div class="grid gap-5 sm:grid-cols-2">
      <TextField name="domain" label="Domain" placeholder="agra.com" />
      <TextField name="website" label="Website" placeholder="https://agra.com" type="url" />
      <TextField name="industry" label="Industry" list="industry-options" />
      <SelectField
        name="size"
        label="Employees"
        none-label="Unknown"
        :options="COMPANY_SIZES.map((s) => ({ value: s, label: s }))"
      />
      <TextField name="phone" label="Phone" type="tel" />
      <TextField name="city" label="City" />
      <TextField name="country" label="Country" />
      <Field v-if="canAssign" :data-invalid="ownerError ? true : undefined">
        <FieldLabel for="company-owner">Owner</FieldLabel>
        <UserSelect id="company-owner" v-model="ownerId" :invalid="Boolean(ownerError)" />
        <FieldError v-if="ownerError">{{ ownerError }}</FieldError>
      </Field>
    </div>
    <datalist id="industry-options">
      <option v-for="i in INDUSTRIES" :key="i" :value="i" />
    </datalist>
  </FormDialog>
</template>
