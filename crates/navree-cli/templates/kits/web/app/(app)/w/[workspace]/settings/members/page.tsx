"use client";

import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MoreHorizontalIcon } from "lucide-react";
import { Controller, useForm } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { FormAlert, TextField } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Field, FieldLabel } from "@/components/ui/field";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api } from "@/lib/api/client";
import { applyFieldErrors, errorMessage } from "@/lib/api/errors";
import type { DataList, Invitation, Member } from "@/lib/api/types";
import { keys, useMe, useWorkspaceSlug, useWorkspaces } from "@/lib/queries";

const INVITABLE = ["admin", "member", "viewer"] as const;

const inviteSchema = z.object({
  email: z.email("Enter an email address."),
  role: z.enum(INVITABLE),
});
type InviteValues = z.infer<typeof inviteSchema>;

const label = (role: string) => role.charAt(0).toUpperCase() + role.slice(1);

/** Invite people by email; they get a link to `/invitations/accept`. */
function InviteForm({ slug }: { slug: string }) {
  const queryClient = useQueryClient();
  const form = useForm<InviteValues>({ resolver: zodResolver(inviteSchema), defaultValues: { email: "", role: "member" } });
  const invite = useMutation({
    mutationFn: (values: InviteValues) => api.post<Invitation>(`/api/workspaces/${slug}/invitations`, values, { workspace: slug }),
    onSuccess: async (invitation) => {
      toast.success(`Invitation sent to ${invitation.email}.`);
      form.reset({ email: "", role: form.getValues("role") });
      await queryClient.invalidateQueries({ queryKey: keys.invitations(slug) });
    },
    onError: (error) => {
      if (!applyFieldErrors(error, form.setError)) {
        form.setError("root.server", { type: "server", message: errorMessage(error) });
      }
    },
    meta: { silent: true },
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle>Invite people</CardTitle>
        <CardDescription>They get an email with a link that is valid for seven days.</CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={form.handleSubmit((values) => invite.mutate(values))} noValidate className="flex flex-col gap-4">
          <FormAlert message={form.formState.errors.root?.server?.message} />
          <div className="grid gap-4 sm:grid-cols-[1fr_10rem]">
            <TextField control={form.control} name="email" label="Email" type="email" />
            <Controller
              control={form.control}
              name="role"
              render={({ field }) => (
                <Field>
                  <FieldLabel htmlFor="invite-role">Role</FieldLabel>
                  <Select value={field.value} onValueChange={field.onChange}>
                    <SelectTrigger id="invite-role">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {INVITABLE.map((role) => (
                        <SelectItem key={role} value={role}>
                          {label(role)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              )}
            />
          </div>
          <Button type="submit" className="self-start" disabled={invite.isPending}>
            {invite.isPending ? <Spinner data-icon="inline-start" /> : null}
            Send invitation
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}

/** The workspace's members, their roles and the pending invitations. */
export default function MembersPage() {
  const slug = useWorkspaceSlug();
  const me = useMe();
  const queryClient = useQueryClient();
  const current = useWorkspaces().data?.find((workspace) => workspace.slug === slug);
  const manager = current?.role === "owner" || current?.role === "admin";
  const owner = current?.role === "owner";

  const members = useQuery({
    queryKey: keys.members(slug),
    queryFn: async () => (await api.get<DataList<Member>>(`/api/workspaces/${slug}/members`, { workspace: slug })).data,
  });
  const invitations = useQuery({
    queryKey: keys.invitations(slug),
    queryFn: async () =>
      (await api.get<DataList<Invitation>>(`/api/workspaces/${slug}/invitations`, { workspace: slug })).data,
    enabled: manager,
  });

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: keys.members(slug) });
    await queryClient.invalidateQueries({ queryKey: keys.workspaces });
  };
  const changeRole = useMutation({
    mutationFn: ({ user, role }: { user: string; role: string }) =>
      api.patch(`/api/workspaces/${slug}/members/${user}`, { role }, { workspace: slug }),
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: (user: string) => api.delete(`/api/workspaces/${slug}/members/${user}`, { workspace: slug }),
    onSuccess: refresh,
  });
  const transfer = useMutation({
    mutationFn: (user: string) => api.post(`/api/workspaces/${slug}/transfer`, { user_id: user }, { workspace: slug }),
    onSuccess: async () => {
      toast.success("Ownership transferred.");
      await refresh();
    },
  });
  const revoke = useMutation({
    mutationFn: (id: string) => api.delete(`/api/workspaces/${slug}/invitations/${id}`, { workspace: slug }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.invitations(slug) }),
  });

  return (
    <>
      <PageHeader title="Members" description="Who is in the workspace, and what their role lets them do." />
      {manager ? <InviteForm slug={slug} /> : null}

      <div className="surface overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Member</TableHead>
              <TableHead>Role</TableHead>
              <TableHead className="w-12">
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {(members.data ?? []).map((member) => {
              const isOwner = member.role === "owner";
              const isMe = member.user_id === me.data?.id;
              return (
                <TableRow key={member.id}>
                  <TableCell>
                    <div className="flex flex-col">
                      <span className="text-fg">
                        {member.user?.name ?? member.user_id}
                        {isMe ? <span className="text-fg-3"> (you)</span> : null}
                      </span>
                      {member.user ? <span className="text-xs text-fg-3">{member.user.email}</span> : null}
                    </div>
                  </TableCell>
                  <TableCell>
                    {manager && !isOwner && !isMe ? (
                      <Select
                        value={member.role}
                        onValueChange={(role) => changeRole.mutate({ user: member.user_id, role })}
                      >
                        <SelectTrigger className="w-32" aria-label={`Role of ${member.user?.name ?? "member"}`}>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {INVITABLE.map((role) => (
                            <SelectItem key={role} value={role}>
                              {label(role)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <Badge variant="outline">{label(member.role)}</Badge>
                    )}
                  </TableCell>
                  <TableCell>
                    {manager && !isOwner && !isMe ? (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label="Member actions">
                            <MoreHorizontalIcon />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          {owner ? (
                            <>
                              <DropdownMenuItem onSelect={() => transfer.mutate(member.user_id)}>
                                Make owner
                              </DropdownMenuItem>
                              <DropdownMenuSeparator />
                            </>
                          ) : null}
                          <DropdownMenuItem variant="destructive" onSelect={() => remove.mutate(member.user_id)}>
                            Remove from workspace
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    ) : null}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>

      {manager && invitations.data && invitations.data.length > 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>Pending invitations</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col divide-y">
            {invitations.data.map((invitation) => (
              <div key={invitation.id} className="flex items-center justify-between gap-4 py-2">
                <div className="flex flex-col">
                  <span className="text-fg">{invitation.email}</span>
                  <span className="text-xs text-fg-3">
                    {label(invitation.role)}, expires {new Date(invitation.expires_at).toLocaleDateString()}
                  </span>
                </div>
                <Button variant="ghost" size="sm" onClick={() => revoke.mutate(invitation.id)}>
                  Revoke
                </Button>
              </div>
            ))}
          </CardContent>
        </Card>
      ) : null}
    </>
  );
}
