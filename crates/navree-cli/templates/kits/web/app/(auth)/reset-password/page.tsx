"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";

const schema = z.object({ password: z.string().min(8, "Use at least 8 characters.") });
type Values = z.infer<typeof schema>;

/** The page the reset mail links to (`auth.password_reset.url`): `?token=..&email=..`. */
function ResetPassword() {
  const params = useSearchParams();
  const token = params.get("token") ?? "";
  const email = params.get("email") ?? "";
  const [done, setDone] = useState(false);
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { password: "" } });

  const onSubmit = form.handleSubmit(async ({ password }) => {
    try {
      await api.post("/api/auth/reset-password", { email, token, password }, { workspace: null });
      setDone(true);
    } catch (error) {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    }
  });

  if (done) {
    return (
      <AuthShell title="Your password is changed" description="Sign in with the new one.">
        <Button size="lg" asChild>
          <Link href="/sign-in">Sign in</Link>
        </Button>
      </AuthShell>
    );
  }

  return (
    <AuthShell title="Choose a new password" description={email ? `For ${email}.` : undefined}>
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        <FormAlert
          message={form.formState.errors.root?.server?.message ?? (token ? undefined : "This link is incomplete.")}
        />
        <TextField
          control={form.control}
          name="password"
          label="New password"
          type="password"
          autoComplete="new-password"
          autoFocus
          description="At least 8 characters."
        />
        <Button type="submit" size="lg" disabled={form.formState.isSubmitting || !token}>
          {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
          Change password
        </Button>
      </form>
    </AuthShell>
  );
}

export default function ResetPasswordPage() {
  return (
    <Suspense>
      <ResetPassword />
    </Suspense>
  );
}
