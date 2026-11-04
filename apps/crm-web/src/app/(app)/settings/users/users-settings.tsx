"use client";

import { Can, useAbility } from "@navree/next/abilities";
import { zodResolver } from "@hookform/resolvers/zod";
import { LockIcon, UserPlusIcon } from "lucide-react";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { z } from "zod";
import { PageActions } from "@/components/app-shell/shell-slots";
import { roleLabel } from "@/components/app-shell/user-menu";
import { SkeletonRows } from "@/components/data-table";
import { FormDialog } from "@/components/form-dialog";
import { SelectField, TextField } from "@/components/form-fields";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { FieldGroup } from "@/components/ui/field";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { UserAvatar } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import { useCreateUser, useUpdateUser, useUsers } from "@/features/users/queries";
import { applyFieldErrors } from "@/lib/api/form-errors";
import type { Role, User } from "@/lib/api/types";
import { ROLES } from "@/lib/api/types";
import { formatDate } from "@/lib/format";

const ROLE_HELP: Record<Role, string> = {
  admin: "Everything, including users, roles and the audit log",
  manager: "All customer records, reassigning deals, exporting contacts",
  rep: "Reads everything; edits and deletes only their own records",
};

const ROLE_OPTIONS = ROLES.map((r) => ({ value: r, label: roleLabel([r]) }));

const schema = z.object({
  name: z.string().trim().min(1, "Enter their name.").max(255),
  email: z.email("Enter a valid email address."),
  password: z.string().min(8, "Use at least 8 characters."),
  role: z.enum(["admin", "manager", "rep"]),
});
type Values = z.infer<typeof schema>;

function CreateUserDialog() {
  const [open, setOpen] = useState(false);
  const create = useCreateUser();
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { name: "", email: "", password: "", role: "rep" },
  });
  const role = useWatch({ control: form.control, name: "role" });

  return (
    <FormDialog
      open={open}
      onOpenChange={(next) => {
        if (next) form.reset();
        setOpen(next);
      }}
      trigger={
        <Button>
          <UserPlusIcon data-icon="inline-start" />
          Add user
        </Button>
      }
      title="Add a user"
      description="They can sign in straight away with the password you set."
      submitLabel="Add user"
      pending={create.isPending}
      rootError={form.formState.errors.root?.server?.message}
      onSubmit={form.handleSubmit((values) =>
        create.mutate(values, {
          onSuccess: (user) => {
            toast.success(`${user.name} added as ${roleLabel(user.roles).toLowerCase()}`);
            setOpen(false);
          },
          onError: (error) => applyFieldErrors(error, form.setError, ["name", "email", "password", "role"]),
        }),
      )}
    >
      <FieldGroup className="gap-4">
        <TextField control={form.control} name="name" label="Name" required autoFocus />
        <TextField control={form.control} name="email" label="Email" type="email" required autoComplete="off" />
        <TextField
          control={form.control}
          name="password"
          label="Temporary password"
          type="password"
          required
          autoComplete="new-password"
          description="At least 8 characters. Ask them to change it after signing in."
        />
        <SelectField control={form.control} name="role" label="Role" options={ROLE_OPTIONS} />
        <p className="-mt-2 text-xs text-muted-foreground">{ROLE_HELP[role]}</p>
      </FieldGroup>
    </FormDialog>
  );
}

function RoleSelect({ user, isSelf }: { user: User; isSelf: boolean }) {
  const update = useUpdateUser();
  const current = (ROLES.find((r) => user.roles.includes(r)) ?? "rep") as Role;
  if (isSelf) {
    return (
      <span className="inline-flex items-center gap-1.5 text-sm" title="You can't change your own role">
        {roleLabel(user.roles)}
        <LockIcon className="size-3 text-muted-foreground" aria-label="You can't change your own role" />
      </span>
    );
  }
  return (
    <Select
      value={current}
      disabled={update.isPending}
      onValueChange={(role) =>
        update.mutate(
          { id: user.id, role: role as Role },
          { onSuccess: () => toast.success(`${user.name} is now ${roleLabel([role]).toLowerCase()}`) },
        )
      }
    >
      <SelectTrigger size="sm" className="w-32" aria-label={`Role for ${user.name}`}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {ROLE_OPTIONS.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}

export function UsersSettings() {
  const ability = useAbility();
  const me = useCurrentUser();
  const users = useUsers({ per_page: 100 });

  if (!ability.can("create", "User")) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <LockIcon />
          </EmptyMedia>
          <EmptyTitle>Only admins can manage users</EmptyTitle>
          <EmptyDescription>Ask an admin if someone needs an account or a different role.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-base font-semibold text-fg">Users and roles</h2>
          <p className="text-sm text-muted-foreground">
            {users.data ? `${users.data.meta.total} people on the team. ` : ""}A role decides what each person can see
            and change.
          </p>
        </div>
        <PageActions>
          <Can I="create" a="User">
            <CreateUserDialog />
          </Can>
        </PageActions>
      </div>
      <div className="surface overflow-hidden">
        <Table className="text-[13px]">
          <TableHeader>
            <TableRow className="bg-ink-1 hover:bg-ink-1 [&>th]:h-9">
              <TableHead className="pl-4">Name</TableHead>
              <TableHead>Role</TableHead>
              <TableHead className="hidden md:table-cell">Can</TableHead>
              <TableHead className="hidden sm:table-cell">Email verified</TableHead>
              <TableHead className="hidden pr-4 lg:table-cell">Joined</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {users.isPending ? (
              <SkeletonRows
                columns={["", "", "hidden md:table-cell", "hidden sm:table-cell", "hidden lg:table-cell"]}
                rows={4}
              />
            ) : (
              users.data?.data.map((u) => {
                const role = (ROLES.find((r) => u.roles.includes(r)) ?? "rep") as Role;
                return (
                  <TableRow key={u.id} data-testid="user-row">
                    <TableCell className="py-2.5 pl-4">
                      <span className="flex items-center gap-2.5">
                        <UserAvatar name={u.name} id={u.id} size="md" />
                        <span className="flex flex-col">
                          <span className="font-medium">
                            {u.name}
                            {u.id === me.data?.id ? (
                              <span className="font-normal text-muted-foreground"> (you)</span>
                            ) : null}
                          </span>
                          <span className="text-xs text-fg-3">{u.email}</span>
                        </span>
                      </span>
                    </TableCell>
                    <TableCell>
                      <RoleSelect user={u} isSelf={u.id === me.data?.id} />
                    </TableCell>
                    <TableCell className="hidden max-w-72 text-muted-foreground md:table-cell">
                      {ROLE_HELP[role]}
                    </TableCell>
                    <TableCell className="hidden text-fg-3 figures sm:table-cell">
                      {u.email_verified_at ? formatDate(u.email_verified_at) : "Not yet"}
                    </TableCell>
                    <TableCell className="hidden pr-4 text-fg-3 figures lg:table-cell">
                      {formatDate(u.created_at)}
                    </TableCell>
                  </TableRow>
                );
              })
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
