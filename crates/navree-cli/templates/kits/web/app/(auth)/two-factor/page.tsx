"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { errorMessage, isApiError } from "@/lib/api/errors";
import { nextQuery, safeNext } from "@/lib/paths";

const schema = z.object({ code: z.string().trim().min(1, "Enter the code.") });
type Values = z.infer<typeof schema>;

/**
 * The second sign-in step of accounts with two-factor authentication: a code from the
 * authenticator app, or a recovery code. The BFF holds the challenge in an httpOnly cookie.
 */
function TwoFactor() {
  const router = useRouter();
  const next = safeNext(useSearchParams().get("next"));
  const queryClient = useQueryClient();
  const [recovery, setRecovery] = useState(false);
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { code: "" } });

  const onSubmit = form.handleSubmit(async ({ code }) => {
    const value = code.trim();
    try {
      await api.post(
        "/api/auth/2fa/verify",
        recovery ? { recovery_code: value } : { code: value.replace(/\s+/g, "") },
        { workspace: null },
      );
      await queryClient.invalidateQueries();
      router.replace(next);
    } catch (error) {
      form.setValue("code", "");
      if (isApiError(error) && error.code === "two_factor_expired") {
        router.replace(`/sign-in${nextQuery(next)}`);
        return;
      }
      form.setError("root.server", { type: "server", message: errorMessage(error) });
    }
  });

  return (
    <AuthShell
      title="Two-step verification"
      description={
        recovery
          ? "Enter one of the recovery codes you saved when you turned it on."
          : "Enter the 6-digit code from your authenticator app."
      }
    >
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        <FormAlert message={form.formState.errors.root?.server?.message} />
        <TextField
          control={form.control}
          name="code"
          label={recovery ? "Recovery code" : "Authentication code"}
          autoComplete="one-time-code"
          autoFocus
        />
        <div className="flex flex-col gap-2">
          <Button type="submit" size="lg" disabled={form.formState.isSubmitting}>
            {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
            Verify
          </Button>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              setRecovery((value) => !value);
              form.reset({ code: "" });
            }}
          >
            {recovery ? "Use your authenticator app instead" : "Use a recovery code instead"}
          </Button>
        </div>
      </form>
      <Link className="record-link self-start text-sm" href="/sign-in">
        Back to sign in
      </Link>
    </AuthShell>
  );
}

export default function TwoFactorPage() {
  return (
    <Suspense>
      <TwoFactor />
    </Suspense>
  );
}
