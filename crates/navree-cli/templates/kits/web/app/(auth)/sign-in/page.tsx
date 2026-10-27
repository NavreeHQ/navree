"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { FieldGroup, FieldSeparator } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { DataList, Provider, SignInResult } from "@/lib/api/types";
import { nextQuery, safeNext } from "@/lib/paths";
import { keys } from "@/lib/queries";

const schema = z.object({
  email: z.email("Enter your email address."),
  password: z.string().min(1, "Enter your password."),
});
type Values = z.infer<typeof schema>;

/** Why the sign-in page was opened (`?error=`), from the social sign-in routes. */
const ERRORS: Record<string, string> = {
  social_state: "That sign-in link expired. Try again.",
  social_failed: "The provider did not confirm your sign-in. Try again.",
  social_unavailable: "That sign-in option is not available.",
  api_unreachable: "The server is unreachable. Try again in a moment.",
};

function SignIn() {
  const router = useRouter();
  const params = useSearchParams();
  const next = safeNext(params.get("next"));
  const queryClient = useQueryClient();
  const providers = useQuery({
    queryKey: keys.providers,
    queryFn: async () => (await api.get<DataList<Provider>>("/api/v1/account/providers", { workspace: null })).data,
  });
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { email: "", password: "" } });

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const result = await api.post<SignInResult>("/api/auth/login", values, { workspace: null });
      if ("two_factor_required" in result) {
        router.push(`/two-factor?next=${encodeURIComponent(next)}`);
        return;
      }
      await queryClient.invalidateQueries();
      router.replace(next);
    } catch (error) {
      form.setValue("password", "");
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    }
  });

  return (
    <AuthShell title="Sign in" description="Welcome back.">
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        <FormAlert message={form.formState.errors.root?.server?.message ?? ERRORS[params.get("error") ?? ""]} />
        <FieldGroup>
          <TextField control={form.control} name="email" label="Email" type="email" autoComplete="email" autoFocus />
          <TextField control={form.control} name="password" label="Password" type="password" autoComplete="current-password" />
        </FieldGroup>
        <Button type="submit" size="lg" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
          Sign in
        </Button>
      </form>
      {providers.data && providers.data.length > 0 ? (
        <div className="flex flex-col gap-3">
          <FieldSeparator>or</FieldSeparator>
          {providers.data.map((provider) => (
            <Button key={provider.key} variant="outline" size="lg" asChild>
              {/* A full page load: the BFF redirects to the provider. */}
              <a href={`/auth/social/${provider.key}?next=${encodeURIComponent(next)}`}>Continue with {provider.name}</a>
            </Button>
          ))}
        </div>
      ) : null}
      <div className="flex flex-col gap-2 text-sm text-fg-2">
        <Link className="record-link self-start" href="/forgot-password">
          Forgot your password?
        </Link>
        <p>
          New here?{" "}
          <Link className="record-link" href={`/sign-up${nextQuery(next)}`}>
            Create an account
          </Link>
        </p>
      </div>
    </AuthShell>
  );
}

export default function SignInPage() {
  return (
    <Suspense>
      <SignIn />
    </Suspense>
  );
}
