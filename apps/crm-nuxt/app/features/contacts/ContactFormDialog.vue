<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import FormDialog from "@/components/crm/FormDialog.vue";
import RecordCombobox from "@/components/crm/RecordCombobox.vue";
import SelectField from "@/components/crm/SelectField.vue";
import TextField from "@/components/crm/TextField.vue";
import type { PickedRecord } from "@/components/crm/types";
import UserSelect from "@/components/crm/UserSelect.vue";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { CompanyRef, Contact, ContactInput, ContactStatus, ContactUpdateInput } from "@/lib/api/types";
import { CONTACT_STATUSES } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { nullable } from "@/lib/forms/values";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { STATUS_LABEL } from "./status";
import { useCreateContact, useUpdateContact } from "./queries";

const props = defineProps<{
  contact?: Contact;
  /** Pre-selects the company when adding from a company page. */
  company?: CompanyRef;
}>();
const open = defineModel<boolean>("open", { default: false });

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

const STATUS_OPTIONS = CONTACT_STATUSES.map((status) => ({ value: status, label: STATUS_LABEL[status] }));

const FIELDS = {
  first_name: "first_name",
  last_name: "last_name",
  email: "email",
  phone: "phone",
  title: "title",
  status: "status",
  company_id: "company",
  owner_id: "owner_id",
} as const;

const ability = useAbility<AppAbility>();
const canAssign = computed(() => ability.can("manage", "Contact"));
const create = useCreateContact();
const update = useUpdateContact(() => props.contact?.id ?? "");
const rootError = ref<string | null>(null);

function initialValues() {
  const c = props.contact?.company ?? props.company ?? null;
  return {
    first_name: props.contact?.first_name ?? "",
    last_name: props.contact?.last_name ?? "",
    email: props.contact?.email ?? "",
    phone: props.contact?.phone ?? "",
    title: props.contact?.title ?? "",
    status: props.contact?.status ?? ("lead" as ContactStatus),
    company: c ? { id: c.id, label: c.name } : null,
    owner_id: props.contact?.owner_id,
  };
}

const form = useForm({ validationSchema: toTypedSchema(schema), initialValues: initialValues() });
const { value: company, errorMessage: companyError } = useField<PickedRecord | null>("company");
const { value: ownerId, errorMessage: ownerError } = useField<string | undefined>("owner_id");

watch(open, (isOpen) => {
  if (!isOpen) return;
  form.resetForm({ values: initialValues() });
  rootError.value = null;
});

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
  // Valid for both POST and PATCH (PATCH takes no null owner_id).
  const body: ContactInput & ContactUpdateInput = {
    first_name: values.first_name,
    last_name: values.last_name,
    email: nullable(values.email),
    phone: nullable(values.phone),
    title: nullable(values.title),
    status: values.status,
    company_id: values.company?.id ?? null,
    ...(canAssign.value && values.owner_id ? { owner_id: values.owner_id } : {}),
  };
  const onError = (error: unknown) => {
    const root = applyFieldErrors(error, form.setFieldError, FIELDS);
    if (root) rootError.value = root;
  };
  if (props.contact) {
    update.mutate(body, {
      onSuccess: () => {
        toast.success("Contact updated");
        open.value = false;
      },
      onError,
    });
  } else {
    create.mutate(body, {
      onSuccess: (created) => {
        toast.success(`${created.full_name} added`, {
          action: { label: "Open", onClick: () => void navigateTo(`/contacts/${created.id}`) },
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
    :title="contact ? `Edit ${contact.full_name}` : 'New contact'"
    :description="contact ? undefined : 'Add a person you are talking to.'"
    :submit-label="contact ? 'Save changes' : 'Create contact'"
    :pending="create.isPending.value || update.isPending.value"
    :root-error="rootError"
    wide
    @submit="onSubmit"
  >
    <template v-if="$slots.trigger" #trigger><slot name="trigger" /></template>
    <div class="grid gap-5 sm:grid-cols-2">
      <TextField name="first_name" label="First name" required autofocus />
      <TextField name="last_name" label="Last name" required />
      <TextField name="email" label="Email" type="email" />
      <TextField name="phone" label="Phone" type="tel" />
      <TextField name="title" label="Job title" />
      <SelectField
        name="status"
        label="Status"
        :options="STATUS_OPTIONS"
      />
      <Field :data-invalid="companyError ? true : undefined">
        <FieldLabel for="contact-company">Company</FieldLabel>
        <RecordCombobox id="contact-company" v-model="company" type="company" placeholder="No company" :invalid="Boolean(companyError)" />
        <FieldError v-if="companyError">{{ companyError }}</FieldError>
      </Field>
      <Field v-if="canAssign" :data-invalid="ownerError ? true : undefined">
        <FieldLabel for="contact-owner">Owner</FieldLabel>
        <UserSelect id="contact-owner" v-model="ownerId" :invalid="Boolean(ownerError)" />
        <FieldError v-if="ownerError">{{ ownerError }}</FieldError>
      </Field>
    </div>
  </FormDialog>
</template>
