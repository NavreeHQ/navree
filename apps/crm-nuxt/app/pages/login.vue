<script setup lang="ts">
import { errorMessage, isNavreeError } from "@navree/client";
import { useField, useForm } from "vee-validate";
import { z } from "zod";
import NavreeMark from "@/components/brand/NavreeMark.vue";
import NavreeWordmark from "@/components/brand/NavreeWordmark.vue";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldError, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { useCurrentUser, useStartSession } from "@/features/auth/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import { DEAL_STAGES, type User } from "@/lib/api/types";
import { toTypedSchema } from "@/lib/forms/zod-schema";
import { stageColor } from "@/lib/stages";

definePageMeta({ layout: "blank" });
useHead({ title: "Sign in" });

const DEMO_ACCOUNTS = [
  { email: "rep1@navree.test", role: "Sales rep" },
  { email: "rep2@navree.test", role: "Sales rep" },
  { email: "manager@navree.test", role: "Manager" },
  { email: "admin@navree.test", role: "Admin" },
];

const route = useRoute();
const config = useCrmConfig();
// Sign-in, two-factor and the session cookies are the @navree/nuxt module's.
const auth = useNavreeAuth<User>();
const startSession = useStartSession();

/** Only same-site paths are allowed as a destination after signing in. */
const next = computed(() => {
  const value = typeof route.query.next === "string" ? route.query.next : null;
  return value && value.startsWith("/") && !value.startsWith("//") && !value.startsWith("/login") ? value : "/";
});

// Already signed in (a valid cookie): go on to the app.
const current = useCurrentUser();
watch(
  () => current.data.value,
  (user) => {
    if (user) void navigateTo(next.value, { replace: true });
  },
  { immediate: true },
);

const twoFactor = ref(false);
const recovery = ref(false);

// ---- Password step ----
const signingIn = ref(false);
const rootError = ref<string | null>(null);
const passwordForm = useForm({
  validationSchema: toTypedSchema(
    z.object({
      email: z.email("Enter the email address you use for Navree."),
      password: z.string().min(1, "Enter your password."),
    }),
  ),
  initialValues: { email: "", password: "" },
});
const { value: email, errorMessage: emailError } = useField<string>("email");
const { value: password, errorMessage: passwordError } = useField<string>("password");

const signIn = passwordForm.handleSubmit(async (values) => {
  rootError.value = null;
  signingIn.value = true;
  try {
    const result = await auth.signIn(values);
    password.value = "";
    if (result.twoFactor) {
      twoFactor.value = true;
      return;
    }
    startSession(result.user);
    void navigateTo(next.value, { replace: true });
  } catch (error) {
    const root = applyFieldErrors(error, passwordForm.setFieldError, ["email", "password"]);
    if (root !== null) {
      if (root) rootError.value = root;
      return;
    }
    password.value = "";
    rootError.value =
      isNavreeError(error) && error.status === 401
        ? "That email and password don't match an account. Check them and try again."
        : isNavreeError(error) && error.status === 429
          ? "Too many sign-in attempts. Wait a minute, then try again."
          : errorMessage(error);
  } finally {
    signingIn.value = false;
  }
});

function fillDemo(address: string) {
  passwordForm.resetForm({ values: { email: address, password: "password123" } });
  rootError.value = null;
}

// ---- Two-factor step ----
const verifying = ref(false);
const codeError = ref<string | null>(null);
const code = ref("");

async function submitCode() {
  const value = code.value.trim();
  if (!value) {
    codeError.value = "Enter the code.";
    return;
  }
  codeError.value = null;
  verifying.value = true;
  try {
    // The challenge token waits in the BFF's httpOnly cookie; the page sends only the proof.
    const user = await auth.verifyTwoFactor(recovery.value ? { recovery_code: value } : { code: value.replace(/\s+/g, "") });
    startSession(user);
    void navigateTo(next.value, { replace: true });
  } catch (error) {
    code.value = "";
    if (isNavreeError(error) && error.status === 401 && error.messages[0] !== "Invalid two-factor code") {
      // The challenge expired: start over from the password.
      twoFactor.value = false;
      rootError.value = "Your sign-in attempt expired. Sign in again.";
      return;
    }
    codeError.value =
      isNavreeError(error) && error.status === 401
        ? recovery.value
          ? "That recovery code isn't valid or was already used."
          : "That code isn't right. Check your authenticator app and try again."
        : isNavreeError(error) && error.status === 429
          ? "Too many attempts. Wait a minute, then try again."
          : isNavreeError(error) && error.isValidation
            ? (Object.values(error.errors)[0]?.[0] ?? errorMessage(error))
            : errorMessage(error);
  } finally {
    verifying.value = false;
  }
}

const soundings = DEAL_STAGES.filter((s) => s.stage !== "lost");
</script>

<template>
  <div class="grid min-h-dvh lg:grid-cols-[minmax(0,6fr)_minmax(0,5fr)]">
    <!-- Brand panel: the pipeline drawn as depth soundings, the one place the motif appears. -->
    <aside class="relative hidden overflow-hidden border-r border-line bg-ink-0 lg:flex lg:flex-col lg:justify-between lg:p-12">
      <div
        aria-hidden="true"
        class="pointer-events-none absolute inset-0 [background-image:linear-gradient(to_right,rgb(28_36_32/0.55)_1px,transparent_1px),linear-gradient(to_bottom,rgb(28_36_32/0.55)_1px,transparent_1px)] [background-size:48px_48px] [mask-image:radial-gradient(ellipse_70%_60%_at_30%_40%,black,transparent_75%)]"
      />
      <NavreeWordmark class="relative" />
      <div class="relative flex max-w-xl flex-col gap-10">
        <div class="flex flex-col gap-5">
          <p class="font-display text-[2.25rem] leading-[1.08] font-semibold tracking-[-0.03em] text-balance text-fg">
            Every account, deal and follow-up your team is working, in one place.
          </p>
        </div>
        <ol class="flex flex-col gap-2.5" aria-label="Deal stages">
          <li v-for="(s, i) in soundings" :key="s.stage" class="flex items-center gap-4">
            <span
              class="h-5 rounded-r-sm"
              aria-hidden="true"
              :style="{
                width: `${30 + i * 14}%`,
                backgroundColor: stageColor(s.stage),
              }"
            />
            <span class="flex items-baseline gap-2 text-sm">
              <span class="text-fg-1">{{ s.label }}</span>
              <span class="font-mono text-xs text-fg-3">{{ s.probability }}%</span>
            </span>
          </li>
        </ol>
      </div>
      <p class="relative font-mono text-xs text-fg-3">Built on the Navree framework · Nuxt front end</p>
    </aside>

    <main class="flex items-center justify-center px-5 py-12 sm:px-8">
      <div v-if="!twoFactor" class="flex w-full max-w-sm flex-col gap-8">
        <div class="flex flex-col gap-3">
          <NavreeMark class="size-8 text-accent lg:hidden" />
          <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] text-fg">Sign in</h1>
          <p class="text-sm text-fg-2">Use your work email and password.</p>
        </div>

        <form novalidate class="flex flex-col gap-6" @submit.prevent="signIn">
          <Alert v-if="rootError" variant="destructive">
            <AlertDescription>{{ rootError }}</AlertDescription>
          </Alert>
          <div class="flex flex-col gap-5">
            <Field :data-invalid="emailError ? true : undefined">
              <FieldLabel for="email">Email</FieldLabel>
              <Input
                id="email"
                v-model="email"
                type="email"
                placeholder="you@company.com"
                autocomplete="username"
                autofocus
                class="h-10"
                :aria-invalid="emailError ? true : undefined"
              />
              <FieldError v-if="emailError">{{ emailError }}</FieldError>
            </Field>
            <Field :data-invalid="passwordError ? true : undefined">
              <FieldLabel for="password">Password</FieldLabel>
              <Input
                id="password"
                v-model="password"
                type="password"
                autocomplete="current-password"
                class="h-10"
                :aria-invalid="passwordError ? true : undefined"
              />
              <FieldError v-if="passwordError">{{ passwordError }}</FieldError>
            </Field>
          </div>
          <Button type="submit" size="lg" :disabled="signingIn">
            <Spinner v-if="signingIn" />
            Sign in
          </Button>
        </form>

        <section v-if="config.demoAccounts" class="flex flex-col gap-3 border-t border-line pt-6" aria-labelledby="demo-accounts">
          <h2 id="demo-accounts" class="text-sm font-medium text-fg-1">Demo accounts</h2>
          <p class="text-xs text-fg-3">
            {{ config.mocks ? "The mock API is on." : "Seeded by crm-demo." }} Every account uses the password
            <code class="font-mono text-fg-2">password123</code>.
          </p>
          <div class="grid grid-cols-2 gap-2">
            <button
              v-for="a in DEMO_ACCOUNTS"
              :key="a.email"
              type="button"
              class="flex flex-col items-start rounded-md border border-line-2 px-3 py-2 text-left transition-colors hover:border-fg-3/60 hover:bg-ink-2"
              @click="fillDemo(a.email)"
            >
              <span class="text-[13px] font-medium text-fg">{{ a.email.replace("@navree.test", "") }}</span>
              <span class="text-xs text-fg-3">{{ a.role }}</span>
            </button>
          </div>
        </section>
      </div>

      <div v-else class="flex w-full max-w-sm flex-col gap-8">
        <div class="flex flex-col gap-3">
          <NavreeMark class="size-8 text-accent lg:hidden" />
          <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] text-fg">Two-step verification</h1>
          <p class="text-sm text-fg-2">
            {{
              recovery
                ? "Enter one of the recovery codes you saved when you turned on two-step verification."
                : "Enter the 6-digit code from your authenticator app."
            }}
          </p>
        </div>
        <form novalidate class="flex flex-col gap-6" @submit.prevent="submitCode">
          <Field :data-invalid="codeError ? true : undefined">
            <FieldLabel for="code">{{ recovery ? "Recovery code" : "Authentication code" }}</FieldLabel>
            <Input
              id="code"
              v-model="code"
              :inputmode="recovery ? 'text' : 'numeric'"
              autocomplete="one-time-code"
              autofocus
              class="h-10 tracking-[0.2em] figures"
              :aria-invalid="codeError ? true : undefined"
            />
            <FieldError v-if="codeError">{{ codeError }}</FieldError>
          </Field>
          <div class="flex flex-col gap-2">
            <Button type="submit" size="lg" :disabled="verifying">
              <Spinner v-if="verifying" />
              Verify
            </Button>
            <Button
              type="button"
              variant="ghost"
              @click="
                recovery = !recovery;
                code = '';
                codeError = null;
              "
            >
              {{ recovery ? "Use your authenticator app instead" : "Use a recovery code instead" }}
            </Button>
          </div>
        </form>
        <Button type="button" variant="link" class="self-start" @click="twoFactor = false">Back to sign in</Button>
      </div>
    </main>
  </div>
</template>
