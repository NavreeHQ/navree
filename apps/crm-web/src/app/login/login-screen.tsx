"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import { NavreeMark } from "@/components/brand";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { needsTwoFactor, useCurrentUser, useLogin, useVerifyTwoFactor } from "@/features/auth/queries";
import { errorMessage, isApiError } from "@/lib/api/errors";
import { applyFieldErrors } from "@/lib/api/form-errors";
import { DEAL_STAGES } from "@/lib/api/types";
import { env } from "@/lib/env";

const schema = z.object({
  email: z.email("Enter the email address you use for Navree."),
  password: z.string().min(1, "Enter your password."),
});
type Values = z.infer<typeof schema>;

const codeSchema = z.object({
  code: z.string().trim().min(1, "Enter the code."),
});
type CodeValues = z.infer<typeof codeSchema>;

const DEMO_ACCOUNTS = [
  { email: "rep1@navree.test", role: "Sales rep" },
  { email: "rep2@navree.test", role: "Sales rep" },
  { email: "manager@navree.test", role: "Manager" },
  { email: "admin@navree.test", role: "Admin" },
];

/** Only same-site paths are allowed as a post-login destination. */
function safeNext(next: string | null): string {
  return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/login") ? next : "/";
}

/** The stage ramp drawn as depth soundings: the brand motif, shown once, here. */
function Soundings() {
  return (
    <ol className="flex flex-col" aria-label="Deal stages">
      {DEAL_STAGES.filter((s) => s.stage !== "lost").map((s, i) => (
        <li key={s.stage} className="flex items-center gap-4">
          <span
            className="h-7 rounded-r-sm"
            style={{ width: `${34 + i * 13}%`, backgroundColor: `var(--stage-${s.stage})` }}
            aria-hidden="true"
          />
          <span className="flex items-baseline gap-2 text-sm">
            <span className="text-sidebar-accent-foreground">{s.label}</span>
            <span className="text-xs text-sidebar-foreground/60 figures">{s.probability}%</span>
          </span>
        </li>
      ))}
    </ol>
  );
}

/**
 * Second step for accounts with two-factor authentication: a code from the
 * authenticator app, or one of the recovery codes. The BFF holds the challenge.
 */
function TwoFactorStep({ onSignedIn, onBack }: { onSignedIn: () => void; onBack: () => void }) {
  const verify = useVerifyTwoFactor();
  const [recovery, setRecovery] = useState(false);
  const form = useForm<CodeValues>({ resolver: zodResolver(codeSchema), defaultValues: { code: "" } });

  const onSubmit = form.handleSubmit(({ code }) => {
    const value = code.trim();
    verify.mutate(recovery ? { recovery_code: value } : { code: value.replace(/\s+/g, "") }, {
      onSuccess: onSignedIn,
      onError: (error) => {
        form.setValue("code", "");
        if (isApiError(error) && error.status === 401 && error.messages[0] !== "Invalid two-factor code") {
          // The challenge expired (or the BFF lost it): start over from the password.
          onBack();
          return;
        }
        if (applyFieldErrors(error, form.setError, ["code", "recovery_code"])) return;
        form.setError("root.server", {
          type: "server",
          message:
            isApiError(error) && error.status === 401
              ? recovery
                ? "That recovery code isn't valid or was already used."
                : "That code isn't right. Check your authenticator app and try again."
              : isApiError(error) && error.status === 429
                ? "Too many attempts. Wait a minute, then try again."
                : errorMessage(error),
        });
      },
    });
  });

  const rootError = form.formState.errors.root?.server?.message;
  const label = recovery ? "Recovery code" : "Authentication code";

  return (
    <div className="flex w-full max-w-sm flex-col gap-8">
      <div className="flex flex-col gap-2">
        <NavreeMark className="size-7 text-primary lg:hidden" />
        <h1 className="text-[28px] leading-tight font-semibold tracking-[-0.015em] font-condensed">
          Two-step verification
        </h1>
        <p className="text-sm text-muted-foreground">
          {recovery
            ? "Enter one of the recovery codes you saved when you turned on two-step verification."
            : "Enter the 6-digit code from your authenticator app."}
        </p>
      </div>

      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        {rootError ? (
          <Alert variant="destructive">
            <AlertDescription>{rootError}</AlertDescription>
          </Alert>
        ) : null}
        <Controller
          control={form.control}
          name="code"
          render={({ field, fieldState }) => (
            <Field data-invalid={fieldState.invalid || undefined}>
              <FieldLabel htmlFor="code">{label}</FieldLabel>
              <Input
                {...field}
                id="code"
                inputMode={recovery ? "text" : "numeric"}
                autoComplete="one-time-code"
                autoFocus
                aria-invalid={fieldState.invalid || undefined}
                className="h-10 tracking-[0.2em] figures"
              />
              <FieldError errors={[fieldState.error]} />
            </Field>
          )}
        />
        <div className="flex flex-col gap-2">
          <Button type="submit" size="lg" className="h-10" disabled={verify.isPending}>
            {verify.isPending ? <Spinner data-icon="inline-start" /> : null}
            Verify
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="text-muted-foreground"
            onClick={() => {
              setRecovery((r) => !r);
              form.reset({ code: "" });
            }}
          >
            {recovery ? "Use your authenticator app instead" : "Use a recovery code instead"}
          </Button>
        </div>
      </form>
      <Button type="button" variant="link" className="self-start px-0" onClick={onBack}>
        Back to sign in
      </Button>
    </div>
  );
}

export function LoginScreen() {
  const router = useRouter();
  const params = useSearchParams();
  const next = safeNext(params.get("next"));
  const current = useCurrentUser();
  const login = useLogin();
  const [twoFactor, setTwoFactor] = useState(false);

  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { email: "", password: "" },
  });

  useEffect(() => {
    if (current.data) router.replace(next);
  }, [current.data, next, router]);

  const onSubmit = form.handleSubmit((values) => {
    login.mutate(values, {
      onSuccess: (result) => {
        if (needsTwoFactor(result)) {
          form.setValue("password", "");
          setTwoFactor(true);
          return;
        }
        router.replace(next);
      },
      onError: (error) => {
        if (applyFieldErrors(error, form.setError, ["email", "password"])) return;
        form.setValue("password", "");
        form.setError("root.server", {
          type: "server",
          message:
            isApiError(error) && error.status === 401
              ? "That email and password don't match an account. Check them and try again."
              : isApiError(error) && error.status === 429
                ? "Too many sign-in attempts. Wait a minute, then try again."
                : errorMessage(error),
        });
      },
    });
  });

  const fillDemo = (email: string) => {
    form.reset({ email, password: "password123" });
  };

  const rootError = form.formState.errors.root?.server?.message;

  return (
    <div className="grid min-h-svh lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
      <aside className="relative hidden flex-col justify-between overflow-hidden bg-sidebar p-10 text-sidebar-foreground lg:flex">
        <div className="flex items-center gap-2.5 text-sidebar-accent-foreground">
          <NavreeMark className="size-6 text-sidebar-primary" />
          <span className="text-xl font-semibold tracking-tight font-condensed">Navree CRM</span>
        </div>
        <div className="flex flex-col gap-8">
          <p className="max-w-sm text-[28px] leading-[1.15] font-medium tracking-[-0.01em] text-sidebar-accent-foreground font-condensed">
            Every account, deal and follow-up your team is working, in one place.
          </p>
          <Soundings />
        </div>
        <p className="text-xs text-sidebar-foreground/60">Built on the Navree framework.</p>
      </aside>

      <main className="flex items-center justify-center px-6 py-12">
        {twoFactor ? (
          <TwoFactorStep onSignedIn={() => router.replace(next)} onBack={() => setTwoFactor(false)} />
        ) : (
          <div className="flex w-full max-w-sm flex-col gap-8">
            <div className="flex flex-col gap-2">
              <NavreeMark className="size-7 text-primary lg:hidden" />
              <h1 className="text-[28px] leading-tight font-semibold tracking-[-0.015em] font-condensed">Sign in</h1>
              <p className="text-sm text-muted-foreground">Use your work email and password.</p>
            </div>

            <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
              {rootError ? (
                <Alert variant="destructive">
                  <AlertDescription>{rootError}</AlertDescription>
                </Alert>
              ) : null}
              <FieldGroup className="gap-5">
                <Controller
                  control={form.control}
                  name="email"
                  render={({ field, fieldState }) => (
                    <Field data-invalid={fieldState.invalid || undefined}>
                      <FieldLabel htmlFor="email">Email</FieldLabel>
                      <Input
                        {...field}
                        id="email"
                        type="email"
                        autoComplete="username"
                        autoFocus
                        aria-invalid={fieldState.invalid || undefined}
                        className="h-10"
                      />
                      <FieldError errors={[fieldState.error]} />
                    </Field>
                  )}
                />
                <Controller
                  control={form.control}
                  name="password"
                  render={({ field, fieldState }) => (
                    <Field data-invalid={fieldState.invalid || undefined}>
                      <FieldLabel htmlFor="password">Password</FieldLabel>
                      <Input
                        {...field}
                        id="password"
                        type="password"
                        autoComplete="current-password"
                        aria-invalid={fieldState.invalid || undefined}
                        className="h-10"
                      />
                      <FieldError errors={[fieldState.error]} />
                    </Field>
                  )}
                />
              </FieldGroup>
              <Button type="submit" size="lg" className="h-10" disabled={login.isPending}>
                {login.isPending ? <Spinner data-icon="inline-start" /> : null}
                Sign in
              </Button>
            </form>

            {env.mocks ? (
              <section className="flex flex-col gap-3 border-t pt-6" aria-labelledby="demo-accounts">
                <h2 id="demo-accounts" className="text-sm font-medium">
                  Demo accounts
                </h2>
                <p className="text-xs text-muted-foreground">
                  The mock API is on. Every account uses the password password123.
                </p>
                <div className="grid grid-cols-2 gap-2">
                  {DEMO_ACCOUNTS.map((a) => (
                    <Button
                      key={a.email}
                      type="button"
                      variant="outline"
                      className="h-auto flex-col items-start gap-0 px-3 py-2 text-left"
                      onClick={() => fillDemo(a.email)}
                    >
                      <span className="text-[13px] font-medium">{a.email.replace("@navree.test", "")}</span>
                      <span className="text-xs font-normal text-muted-foreground">{a.role}</span>
                    </Button>
                  ))}
                </div>
              </section>
            ) : null}
          </div>
        )}
      </main>
    </div>
  );
}
