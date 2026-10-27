<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { Lock } from "@lucide/vue";
import { toast } from "vue-sonner";
import SkeletonRows from "@/components/crm/SkeletonRows.vue";
import UserAvatar from "@/components/crm/UserAvatar.vue";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { useCurrentUser } from "@/features/auth/queries";
import CreateUserDialog from "@/features/users/CreateUserDialog.vue";
import { useUpdateUser, useUsers } from "@/features/users/queries";
import type { Role, User } from "@/lib/api/types";
import { ROLES } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { formatDate } from "@/lib/format";
import { ROLE_HELP, roleLabel } from "@/lib/roles";

useHead({ title: "Users and roles" });

const ability = useAbility<AppAbility>();
const me = useCurrentUser();
const users = useUsers({ per_page: 100 });
const update = useUpdateUser();
const canManage = computed(() => ability.can("create", "User"));

function roleOf(user: User): Role {
  return (ROLES.find((r) => user.roles.includes(r)) ?? "rep") as Role;
}

function changeRole(user: User, role: unknown) {
  if (typeof role !== "string" || role === roleOf(user)) return;
  update.mutate(
    { id: user.id, role: role as Role },
    { onSuccess: () => toast.success(`${user.name} is now ${roleLabel([role]).toLowerCase()}`) },
  );
}
</script>

<template>
  <Empty v-if="!canManage" class="surface border">
    <EmptyHeader>
      <EmptyMedia variant="icon"><Lock /></EmptyMedia>
      <EmptyTitle>Only admins can manage users</EmptyTitle>
      <EmptyDescription>Ask an admin if someone needs an account or a different role.</EmptyDescription>
    </EmptyHeader>
  </Empty>
  <div v-else class="flex flex-col gap-4">
    <div class="flex flex-wrap items-end justify-between gap-3">
      <div class="flex flex-col gap-1">
        <h2 class="text-base font-semibold text-fg">Users and roles</h2>
        <p class="text-[13px] text-fg-2">
          <template v-if="users.data.value">{{ users.data.value.meta.total }} people on the team. </template>A role decides what each person can see and change.
        </p>
      </div>
      <CreateUserDialog />
    </div>
    <section class="surface overflow-hidden" aria-label="Users">
      <div class="overflow-x-auto scroll-thin">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead class="pl-4">Name</TableHead>
              <TableHead>Role</TableHead>
              <TableHead class="hidden md:table-cell">Can</TableHead>
              <TableHead class="max-sm:hidden">Email verified</TableHead>
              <TableHead class="pr-4 max-sm:hidden">Joined</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <SkeletonRows v-if="users.isPending.value" :columns="5" :rows="4" />
            <TableRow v-for="u in users.data.value?.data ?? []" v-else :key="u.id" data-testid="user-row">
              <TableCell class="py-2.5 pl-4">
                <span class="flex items-center gap-2.5">
                  <UserAvatar :name="u.name" :id="u.id" size="sm" />
                  <span class="flex min-w-0 flex-col">
                    <span class="font-medium text-fg">
                      {{ u.name }}<span v-if="u.id === me.data.value?.id" class="font-normal text-fg-3"> (you)</span>
                    </span>
                    <span class="truncate text-xs text-fg-3">{{ u.email }}</span>
                  </span>
                </span>
              </TableCell>
              <TableCell>
                <span v-if="u.id === me.data.value?.id" class="inline-flex items-center gap-1.5 text-fg-1" title="You can't change your own role">
                  {{ roleLabel(u.roles) }}
                  <Lock class="size-3 text-fg-3" aria-label="You can't change your own role" />
                </span>
                <Select v-else :model-value="roleOf(u)" :disabled="update.isPending.value" @update:model-value="(v) => changeRole(u, v)">
                  <SelectTrigger size="sm" class="w-32" :aria-label="`Role for ${u.name}`">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem v-for="r in ROLES" :key="r" :value="r">{{ roleLabel([r]) }}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </TableCell>
              <TableCell class="hidden max-w-72 whitespace-normal text-fg-2 md:table-cell">{{ ROLE_HELP[roleOf(u)] }}</TableCell>
              <TableCell class="text-fg-3 figures max-sm:hidden">{{ u.email_verified_at ? formatDate(u.email_verified_at) : "Not yet" }}</TableCell>
              <TableCell class="pr-4 text-fg-3 figures max-sm:hidden">{{ formatDate(u.created_at) }}</TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </section>
  </div>
</template>
