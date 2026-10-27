<script setup lang="ts">
import { UserPlus } from "@lucide/vue";
import { useField, useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import FormDialog from "@/components/crm/FormDialog.vue";
import SelectField from "@/components/crm/SelectField.vue";
import TextField from "@/components/crm/TextField.vue";
import { Button } from "@/components/ui/button";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { Role } from "@/lib/api/types";
import { ROLES } from "@/lib/api/types";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { ROLE_HELP, roleLabel } from "@/lib/roles";
import { useCreateUser } from "./queries";

const open = ref(false);
const create = useCreateUser();
const rootError = ref<string | null>(null);
const form = useForm({
  validationSchema: toTypedSchema(
    z.object({
      name: z.string().trim().min(1, "Enter their name.").max(255),
      email: z.email("Enter a valid email address."),
      password: z.string().min(8, "Use at least 8 characters."),
      role: z.enum(["admin", "manager", "rep"]),
    }),
  ),
  initialValues: { name: "", email: "", password: "", role: "rep" as Role },
});
const { value: role } = useField<Role>("role");

watch(open, (isOpen) => {
  if (!isOpen) return;
  form.resetForm();
  rootError.value = null;
});

const onSubmit = form.handleSubmit((values) => {
  rootError.value = null;
  create.mutate(values, {
    onSuccess: (user) => {
      toast.success(`${user.name} added as ${roleLabel(user.roles).toLowerCase()}`);
      open.value = false;
    },
    onError: (error) => {
      const root = applyFieldErrors(error, form.setFieldError, ["name", "email", "password", "role"]);
      if (root) rootError.value = root;
    },
  });
});
</script>

<template>
  <FormDialog
    v-model:open="open"
    title="Add a user"
    description="They can sign in straight away with the password you set."
    submit-label="Add user"
    :pending="create.isPending.value"
    :root-error="rootError"
    @submit="onSubmit"
  >
    <template #trigger>
      <Button><UserPlus /> Add user</Button>
    </template>
    <TextField name="name" label="Name" required autofocus />
    <TextField name="email" label="Email" type="email" required autocomplete="off" />
    <TextField
      name="password"
      label="Temporary password"
      type="password"
      required
      autocomplete="new-password"
      description="At least 8 characters. Ask them to change it after signing in."
    />
    <div class="flex flex-col gap-2">
      <SelectField name="role" label="Role" :options="ROLES.map((r) => ({ value: r, label: roleLabel([r]) }))" />
      <p class="text-xs text-fg-3">{{ ROLE_HELP[role] }}</p>
    </div>
  </FormDialog>
</template>
