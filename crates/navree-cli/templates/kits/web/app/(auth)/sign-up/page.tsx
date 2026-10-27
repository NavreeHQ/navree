"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { FieldGroup } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import { nextQuery, safeNext } from "@/lib/paths";

const schema = z.object({
  name: z.string().trim().min(1, "Enter your name.").max(100),
  email: z.email("Enter your email address."),
  password: z.string().min(8, "Use at least 8 characters."),
});
type Values = z.infer<typeof schema>;

function SignUp() {
  const router = useRouter();
  const next = safeNext(useSearchParams().get("next"));
  const queryClient = useQueryClient();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "", email: "", password: "" } });

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      // The BFF keeps the new account's tokens in httpOnly cookies and returns only {user}.
      await api.post("/api/auth/register", values, { workspace: null });
      await api.post("/api/auth/email/verification", undefined, { workspace: null });
      await queryClient.invalidateQueries();
      router.replace(`/verify-email${nextQuery(next)}`);
    } catch (error) {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    }
  });

  return (
    <AuthShell title="Create your account" description="Then create a workspace for your team.">
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        <FormAlert message={form.formState.errors.root?.server?.message} />
        <FieldGroup>
          <TextField control={form.control} name="name" label="Name" autoComplete="name" autoFocus />
          <TextField control={form.control} name="email" label="Email" type="email" autoComplete="email" />
          <TextField
            control={form.control}
            name="password"
            label="Password"
            type="password"
            autoComplete="new-password"
            description="At least 8 characters."
          />
        </FieldGroup>
        <Button type="submit" size="lg" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
          Create account
        </Button>
      </form>
      <p className="text-sm text-fg-2">
        Already have an account?{" "}
        <Link className="record-link" href={`/sign-in${nextQuery(next)}`}>
          Sign in
        </Link>
      </p>
    </AuthShell>
  );
}

export default function SignUpPage() {
  return (
    <Suspense>
      <SignUp />
    </Suspense>
  );
}
