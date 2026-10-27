"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PlusIcon } from "lucide-react";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { FieldGroup } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, query } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { Project, ProjectPage } from "@/lib/api/types";
import { keys, useWorkspaceAbility, useWorkspaceSlug } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

const PER_PAGE = 20;

const schema = z.object({
  name: z.string().trim().min(1, "Name the project.").max(120),
  description: z.string().max(2000),
});
type Values = z.infer<typeof schema>;

/** "New project": the form in a dialog. */
function NewProject({ slug }: { slug: string }) {
  const [open, setOpen] = useState(false);
  const queryClient = useQueryClient();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: { name: "", description: "" } });
  const create = useMutation({
    mutationFn: (values: Values) =>
      api.post<Project>("/api/v1/projects", { name: values.name, description: values.description || null }, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.projects(slug) });
      form.reset();
      setOpen(false);
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button>
          <PlusIcon data-icon="inline-start" />
          New project
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={form.handleSubmit((values) => create.mutate(values))} noValidate className="flex flex-col gap-6">
          <DialogHeader>
            <DialogTitle>New project</DialogTitle>
            <DialogDescription>Everyone in the workspace can see it.</DialogDescription>
          </DialogHeader>
          <FormAlert message={form.formState.errors.root?.server?.message} />
          <FieldGroup>
            <TextField control={form.control} name="name" label="Name" autoFocus />
            <TextField control={form.control} name="description" label="Description" multiline />
          </FieldGroup>
          <DialogFooter>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? <Spinner data-icon="inline-start" /> : null}
              Create project
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The workspace's projects, with filters and pages kept in the URL. */
function Projects() {
  const slug = useWorkspaceSlug();
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const ability = useWorkspaceAbility(slug);
  const status = params.get("status") ?? "all";
  const page = Math.max(1, Number(params.get("page") ?? 1) || 1);
  const [search, setSearch] = useState(params.get("search") ?? "");

  const setParams = (changes: Record<string, string | number | null>) => {
    const next = new URLSearchParams(params);
    for (const [key, value] of Object.entries(changes)) {
      if (value === null || value === "" || value === "all") next.delete(key);
      else next.set(key, String(value));
    }
    const text = next.toString();
    router.replace(`${pathname}${text ? `?${text}` : ""}`);
  };

  const filters = { status: status === "all" ? null : status, search: params.get("search"), page, per_page: PER_PAGE };
  const projects = useQuery({
    queryKey: [...keys.projects(slug), filters],
    queryFn: () => api.get<ProjectPage>(`/api/v1/projects${query(filters)}`, { workspace: slug }),
    placeholderData: keepPreviousData,
  });
  const meta = projects.data?.meta;

  return (
    <>
      <PageHeader
        title="Projects"
        description="Everything your team is working on."
        actions={ability.can("create", "Project") ? <NewProject slug={slug} /> : null}
      />
      <div className="flex flex-wrap items-center gap-2">
        <form
          className="flex-1"
          onSubmit={(event) => {
            event.preventDefault();
            setParams({ search, page: null });
          }}
        >
          <Input
            type="search"
            placeholder="Search by name"
            aria-label="Search projects"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            className="max-w-xs"
          />
        </form>
        <Select value={status} onValueChange={(value) => setParams({ status: value, page: null })}>
          <SelectTrigger className="w-40" aria-label="Status">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All projects</SelectItem>
            <SelectItem value="active">Active</SelectItem>
            <SelectItem value="archived">Archived</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {projects.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="h-10" />
          ))}
        </div>
      ) : projects.data && projects.data.data.length > 0 ? (
        <div className="surface overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {projects.data.data.map((project) => (
                <TableRow key={project.id}>
                  <TableCell>
                    <Link className="record-link" href={workspacePath(slug, `/projects/${project.id}`)}>
                      {project.name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <StatusBadge status={project.status} />
                  </TableCell>
                  <TableCell className="text-right text-fg-2 figures">
                    {new Date(project.updated_at).toLocaleDateString()}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      ) : (
        <Empty className="surface">
          <EmptyHeader>
            <EmptyTitle>No projects here</EmptyTitle>
            <EmptyDescription>
              {status !== "all" || params.get("search") ? "Nothing matches these filters." : "Create the first one."}
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      )}

      {meta && meta.last_page > 1 ? (
        <nav className="flex items-center justify-between text-sm text-fg-2" aria-label="Pages">
          <span className="figures">
            Page {meta.current_page} of {meta.last_page} ({meta.total} projects)
          </span>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setParams({ page: page - 1 })}>
              Previous
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={page >= meta.last_page}
              onClick={() => setParams({ page: page + 1 })}
            >
              Next
            </Button>
          </div>
        </nav>
      ) : null}
    </>
  );
}

export default function ProjectsPage() {
  return (
    <Suspense>
      <Projects />
    </Suspense>
  );
}
