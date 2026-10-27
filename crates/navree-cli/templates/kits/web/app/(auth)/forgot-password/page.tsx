"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import Link from "next/link";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";

const schema = z.object({ email: z.email("Enter your email address.") });
type Values = z.infer<typeof schema>;

export default function ForgotPasswordPage() {
  const [sent, setSent] = useState(false);
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { email: "" } });

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      // The API answers the same whether or not the address has an account.
      await api.post("/api/auth/forgot-password", values, { workspace: null });
      setSent(true);
    } catch (error) {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    }
  });

  return (
    <AuthShell
      title="Reset your password"
      description="We'll email you a link to choose a new one."
    >
      {sent ? (
        <FormAlert variant="default" message="If that address has an account, a reset link is on its way." />
      ) : (
        <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
          <FormAlert message={form.formState.errors.root?.server?.message} />
          <TextField control={form.control} name="email" label="Email" type="email" autoComplete="email" autoFocus />
          <Button type="submit" size="lg" disabled={form.formState.isSubmitting}>
            {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
            Send the link
          </Button>
        </form>
      )}
      <Link className="record-link self-start text-sm" href="/sign-in">
        Back to sign in
      </Link>
    </AuthShell>
  );
}
