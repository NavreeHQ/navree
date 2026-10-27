"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { FieldGroup } from "@/components/ui/field";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { WorkspaceSettings } from "@/lib/api/types";
import { keys, useWorkspaceSlug, useWorkspaces } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

const schema = z.object({
  name: z.string().trim().min(1, "Name the workspace.").max(100),
  slug: z
    .string()
    .min(1)
    .max(64)
    .regex(/^[a-z0-9]+(-[a-z0-9]+)*$/, "Lower-case letters, digits and single dashes."),
});
type Values = z.infer<typeof schema>;

/** The workspace's name and slug (its URL), and leaving or deleting it. */
export default function GeneralSettingsPage() {
  const slug = useWorkspaceSlug();
  const router = useRouter();
  const queryClient = useQueryClient();
  const current = useWorkspaces().data?.find((workspace) => workspace.slug === slug);
  const canChange = current?.role === "owner" || current?.role === "admin";
  const settings = useQuery({
    queryKey: keys.settings(slug),
    queryFn: () => api.get<WorkspaceSettings>("/api/v1/settings/workspace", { workspace: slug }),
  });
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "", slug: "" } });
  const { reset } = form;
  const typedSlug = useWatch({ control: form.control, name: "slug" });
  useEffect(() => {
    if (settings.data) reset({ name: settings.data.name, slug: settings.data.slug });
  }, [settings.data, reset]);

  const save = useMutation({
    mutationFn: (values: Values) => api.patch<WorkspaceSettings>("/api/v1/settings/workspace", values, { workspace: slug }),
    onSuccess: async (saved) => {
      await queryClient.invalidateQueries({ queryKey: keys.workspaces });
      if (saved.slug !== slug) router.replace(workspacePath(saved.slug, "/settings"));
      else await queryClient.invalidateQueries({ queryKey: keys.settings(slug) });
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });
  const leave = useMutation({
    mutationFn: () => api.post(`/api/workspaces/${slug}/leave`, undefined, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.workspaces });
      router.replace("/");
    },
  });
  const destroy = useMutation({
    mutationFn: () => api.delete(`/api/workspaces/${slug}`, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.workspaces });
      router.replace("/");
    },
  });

  return (
    <>
      <PageHeader title="General" description="The workspace's name, and its address in the app's URLs." />
      <Card>
        <CardContent>
          <form onSubmit={form.handleSubmit((values) => save.mutate(values))} noValidate className="flex flex-col gap-6">
            <FormAlert message={form.formState.errors.root?.server?.message} />
            <fieldset disabled={!canChange} className="contents">
              <FieldGroup>
                <TextField control={form.control} name="name" label="Name" />
                <TextField
                  control={form.control}
                  name="slug"
                  label="Slug"
                  description={`Links to this workspace start with /w/${typedSlug || slug}/.`}
                />
              </FieldGroup>
            </fieldset>
            {canChange ? (
              <Button type="submit" className="self-start" disabled={save.isPending || !form.formState.isDirty}>
                {save.isPending ? <Spinner data-icon="inline-start" /> : null}
                Save changes
              </Button>
            ) : null}
          </form>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{current?.role === "owner" ? "Delete the workspace" : "Leave the workspace"}</CardTitle>
          <CardDescription>
            {current?.role === "owner"
              ? "Its projects, members and invitations are deleted. Transfer it to someone else first to keep it."
              : "You lose access to its projects until someone invites you again."}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="destructive">{current?.role === "owner" ? "Delete workspace" : "Leave workspace"}</Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{current?.role === "owner" ? "Delete this workspace?" : "Leave this workspace?"}</AlertDialogTitle>
                <AlertDialogDescription>This can&apos;t be undone.</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  onClick={() => (current?.role === "owner" ? destroy.mutate() : leave.mutate())}
                >
                  {current?.role === "owner" ? "Delete" : "Leave"}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardContent>
      </Card>
    </>
  );
}
