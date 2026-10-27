"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect } from "react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { StatusBadge } from "@/components/status-badge";
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
import { Card, CardContent } from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { Project } from "@/lib/api/types";
import { keys, useWorkspaceAbility, useWorkspaceSlug } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

const schema = z.object({
  name: z.string().trim().min(1, "Name the project.").max(120),
  description: z.string().max(2000),
  status: z.enum(["active", "archived"]),
});
type Values = z.infer<typeof schema>;

/** One project: its details, editable by whoever the rules let edit it. */
export default function ProjectPage() {
  const slug = useWorkspaceSlug();
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  const queryClient = useQueryClient();
  const ability = useWorkspaceAbility(slug);
  const project = useQuery({
    queryKey: keys.project(slug, id),
    queryFn: () => api.get<Project>(`/api/v1/projects/${id}`, { workspace: slug }),
  });
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", description: "", status: "active" },
  });
  const { reset } = form;
  useEffect(() => {
    if (project.data) {
      reset({
        name: project.data.name,
        description: project.data.description ?? "",
        status: project.data.status === "archived" ? "archived" : "active",
      });
    }
  }, [project.data, reset]);

  const update = useMutation({
    mutationFn: (values: Values) => api.patch<Project>(`/api/v1/projects/${id}`, values, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.projects(slug) });
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });
  const remove = useMutation({
    mutationFn: () => api.delete(`/api/v1/projects/${id}`, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.projects(slug) });
      router.replace(workspacePath(slug, "/projects"));
    },
  });

  if (project.isPending) return <Skeleton className="h-48" />;
  if (!project.data) {
    return (
      <PageHeader
        title="Project not found"
        description={<Link className="record-link" href={workspacePath(slug, "/projects")}>Back to the projects</Link>}
      />
    );
  }

  // The API sends `__type: "Project"`, so the rules' conditions (created_by) apply to it.
  const canEdit = ability.can("update", project.data);
  const canDelete = ability.can("delete", project.data);

  return (
    <>
      <PageHeader
        title={project.data.name}
        description={<StatusBadge status={project.data.status} />}
        actions={
          canDelete ? (
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="destructive">Delete</Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete this project?</AlertDialogTitle>
                  <AlertDialogDescription>It is removed for everyone in the workspace.</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={() => remove.mutate()}>Delete</AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          ) : null
        }
      />
      <Card className="max-w-2xl">
        <CardContent>
          <form onSubmit={form.handleSubmit((values) => update.mutate(values))} noValidate className="flex flex-col gap-6">
            <FormAlert message={form.formState.errors.root?.server?.message} />
            <fieldset disabled={!canEdit} className="contents">
              <FieldGroup>
                <TextField control={form.control} name="name" label="Name" />
                <TextField control={form.control} name="description" label="Description" multiline />
                <Controller
                  control={form.control}
                  name="status"
                  render={({ field }) => (
                    <Field>
                      <FieldLabel htmlFor="status">Status</FieldLabel>
                      <Select value={field.value} onValueChange={field.onChange} disabled={!canEdit}>
                        <SelectTrigger id="status" className="w-48">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="active">Active</SelectItem>
                          <SelectItem value="archived">Archived</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  )}
                />
              </FieldGroup>
            </fieldset>
            {canEdit ? (
              <Button type="submit" className="self-start" disabled={update.isPending || !form.formState.isDirty}>
                {update.isPending ? <Spinner data-icon="inline-start" /> : null}
                Save changes
              </Button>
            ) : (
              <p className="text-sm text-fg-3">You can read this project; your role doesn&apos;t let you change it.</p>
            )}
          </form>
        </CardContent>
      </Card>
    </>
  );
}
