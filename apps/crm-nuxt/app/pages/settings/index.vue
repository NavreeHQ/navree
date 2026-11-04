<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Keyboard } from "@lucide/vue";
import { errorMessage, isNavreeError } from "@navree/client";
import { useForm } from "vee-validate";
import { toast } from "vue-sonner";
import { z } from "zod";
import DetailRow from "@/components/crm/DetailRow.vue";
import Panel from "@/components/crm/Panel.vue";
import TextField from "@/components/crm/TextField.vue";
import UserAvatar from "@/components/crm/UserAvatar.vue";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { useChangePassword, useCurrentUser } from "@/features/auth/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { SubjectName } from "@/lib/api/types";
import type { AppAbility, AppAction } from "@/lib/casl/ability";
import { formatDateLong } from "@/lib/format";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { roleLabel } from "@/lib/roles";
import { useUiStore } from "@/stores/ui";

useHead({ title: "Settings" });

const me = useCurrentUser();
const ability = useAbility<AppAbility>();
const config = useCrmConfig();
const ui = useUiStore();

const PERMISSION_ROWS: { subject: SubjectName; label: string }[] = [
  { subject: "Company", label: "Companies" },
  { subject: "Contact", label: "Contacts" },
  { subject: "Deal", label: "Deals" },
  { subject: "Activity", label: "Activities and tasks" },
  { subject: "User", label: "Users" },
];

/** What the current rules allow, so people know why a button is missing. */
function describe(subject: SubjectName): string {
  const can = (action: AppAction) => ability.can(action, subject);
  const all = (action: AppAction) => ability.rulesFor(action, subject).some((r) => !r.inverted && !r.conditions);
  if (ability.can("manage", subject) && all("manage")) return "Full access";
  const parts: string[] = [];
  if (can("read")) parts.push("view");
  if (can("create")) parts.push("create");
  if (can("update")) parts.push(all("update") ? "edit any" : "edit your own");
  if (subject === "Contact" && can("export")) parts.push("export");
  if (subject === "Deal" && can("move")) parts.push(all("move") ? "move any" : "move your own");
  if (can("delete")) parts.push(all("delete") ? "delete any" : "delete your own");
  if (parts.length === 0) return "No access";
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// ---- Change password: the BFF swaps the new token pair into the cookies ----
const changePassword = useChangePassword();
const passwordError = ref<string | null>(null);
const passwordForm = useForm({
  validationSchema: toTypedSchema(
    z
      .object({
        current_password: z.string().min(1, "Enter your current password."),
        password: z.string().min(8, "Use at least 8 characters."),
        confirm: z.string(),
      })
      .refine((v) => v.password === v.confirm, { path: ["confirm"], message: "The passwords don't match." }),
  ),
  initialValues: { current_password: "", password: "", confirm: "" },
});
const savePassword = passwordForm.handleSubmit((values) => {
  passwordError.value = null;
  changePassword.mutate(
    { current_password: values.current_password, password: values.password },
    {
      onSuccess: () => {
        toast.success("Password changed. Other devices have been signed out.");
        passwordForm.resetForm();
      },
      onError: (error) => {
        const root = applyFieldErrors(error, passwordForm.setFieldError, ["current_password", "password"]);
        if (root !== null) {
          if (root) passwordError.value = root;
          return;
        }
        passwordError.value =
          isNavreeError(error) && (error.status === 401 || error.status === 403)
            ? "Your current password isn't right."
            : errorMessage(error);
      },
    },
  );
});
</script>

<template>
  <div v-if="me.data.value" class="grid gap-6 lg:grid-cols-2">
    <Panel title="Profile">
      <div class="flex items-center gap-3 pb-4">
        <UserAvatar :name="me.data.value.name" :id="me.data.value.id" size="lg" />
        <div class="flex min-w-0 flex-col">
          <span class="font-medium text-fg">{{ me.data.value.name }}</span>
          <span class="truncate text-[13px] text-fg-3">{{ me.data.value.email }}</span>
        </div>
      </div>
      <dl class="border-t border-line pt-2">
        <DetailRow label="Role" :value="roleLabel(me.data.value.roles)" />
        <DetailRow label="Member since" :value="formatDateLong(me.data.value.created_at)" />
        <DetailRow label="Email verified" :value="me.data.value.email_verified_at ? formatDateLong(me.data.value.email_verified_at) : 'Not yet'" />
      </dl>
      <p class="mt-3 text-xs text-fg-3">Ask an admin to change your name or role.</p>
    </Panel>

    <Panel title="Password">
      <form novalidate class="flex flex-col gap-4" @submit.prevent="savePassword">
        <Alert v-if="passwordError" variant="destructive"><AlertDescription>{{ passwordError }}</AlertDescription></Alert>
        <TextField name="current_password" label="Current password" type="password" autocomplete="current-password" />
        <div class="grid gap-4 sm:grid-cols-2">
          <TextField name="password" label="New password" type="password" autocomplete="new-password" />
          <TextField name="confirm" label="Repeat it" type="password" autocomplete="new-password" />
        </div>
        <div class="flex items-center justify-between gap-3">
          <p class="text-xs text-fg-3">Changing it signs out your other devices.</p>
          <Button type="submit" variant="outline" :disabled="changePassword.isPending.value">
            <Spinner v-if="changePassword.isPending.value" />
            Change password
          </Button>
        </div>
      </form>
    </Panel>

    <Panel title="What you can do">
      <dl>
        <DetailRow v-for="row in PERMISSION_ROWS" :key="row.subject" :label="row.label">{{ describe(row.subject) }}</DetailRow>
      </dl>
      <p class="mt-3 text-xs text-fg-3">Permissions come from your role. Actions you can't take are hidden across the app.</p>
    </Panel>

    <Panel title="Connection">
      <dl>
        <DetailRow label="Data source">
          {{ config.mocks ? "Mock API (demo data in this browser tab)" : "Navree CRM API, through this app's server" }}
        </DetailRow>
        <DetailRow label="Live updates">
          {{ config.mocks ? "Simulated in this tab" : config.realtime ? `WebSocket ${config.realtime.wsHost}:${config.realtime.wsPort}` : "Off" }}
          <span class="ml-1.5 text-fg-3">({{ ui.realtime === "live" ? "connected" : ui.realtime }})</span>
        </DetailRow>
        <DetailRow label="Tokens">Kept in httpOnly cookies on the server; this page never sees them.</DetailRow>
      </dl>
      <Button variant="ghost" size="sm" class="mt-3 -ml-2" @click="ui.shortcutsOpen = true">
        <Keyboard />
        Keyboard shortcuts
      </Button>
    </Panel>
  </div>
</template>
