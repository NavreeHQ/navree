"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert, TextField } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { Workspace } from "@/lib/api/types";
import { keys } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

const schema = z.object({ name: z.string().trim().min(1, "Name your workspace.").max(100) });
type Values = z.infer<typeof schema>;

/** A new account's first workspace (and anyone's next one). */
export default function OnboardingPage() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "" } });

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const workspace = await api.post<Workspace>("/api/workspaces", values, { workspace: null });
      await queryClient.invalidateQueries({ queryKey: keys.workspaces });
      router.replace(workspacePath(workspace.slug));
    } catch (error) {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    }
  });

  return (
    <AuthShell
      title="Create a workspace"
      description="A workspace holds your team's projects, members and billing. You can invite people next."
    >
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-6">
        <FormAlert message={form.formState.errors.root?.server?.message} />
        <TextField control={form.control} name="name" label="Workspace name" autoComplete="organization" autoFocus />
        <Button type="submit" size="lg" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting ? <Spinner data-icon="inline-start" /> : null}
          Create workspace
        </Button>
      </form>
    </AuthShell>
  );
}
