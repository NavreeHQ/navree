<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useActivities } from "@/features/activities/queries";
import type { User } from "@/lib/api/types";
import type { AppAbility } from "@/lib/casl/ability";
import { isActive, SETTINGS_NAV, WORK_NAV } from "@/lib/navigation";
import { cn } from "@/lib/utils";
import NavLink from "./NavLink.vue";

/**
 * The sidebar's navigation, shared by the desktop sidebar and the phone sheet:
 * two groups (Sales, Workspace), each a 32px sentence-case label over its
 * items; Settings carries its sections as sub-items on a guide line. Collapsed
 * to the icon rail, the labels and sub-items fold away.
 */
const props = defineProps<{ user: User; collapsed?: boolean }>();
const emit = defineEmits<{ navigate: [] }>();

const route = useRoute();
const ability = useAbility<AppAbility>();
const canManageUsers = computed(() => ability.can("create", "User"));
const isAdmin = computed(() => props.user.roles.includes("admin"));
const subItems = computed(() =>
  [
    { ...SETTINGS_NAV.users, show: canManageUsers.value },
    { ...SETTINGS_NAV.audit, show: isAdmin.value },
  ].filter((item) => item.show),
);

// The overdue count on Tasks: one row per page is enough, the total is in `meta`.
const overdue = useActivities(() => ({
  filter: { kind: "task", assignee_id: props.user.id, due: "overdue", completed: false },
  per_page: 1,
}));
const overdueCount = computed(() => overdue.data.value?.meta.total ?? 0);
</script>

<template>
  <nav aria-label="Main" :class="cn('flex flex-col', collapsed && 'items-center')">
    <div :class="cn('flex w-full flex-col p-2', collapsed && 'items-center')">
      <p v-if="!collapsed" class="flex h-8 items-center px-2 text-xs text-fg-3">Sales</p>
      <ul class="flex w-full flex-col gap-0.5" :class="collapsed && 'items-center'">
        <li v-for="item in WORK_NAV" :key="item.href" class="w-full" :class="collapsed && 'w-auto'">
          <NavLink
            :href="item.href"
            :label="item.label"
            :icon="item.icon"
            :active="isActive(route.path, item.href)"
            :collapsed="collapsed"
            :count="item.href === '/tasks' ? overdueCount : undefined"
            count-label="overdue"
            @navigate="emit('navigate')"
          />
        </li>
      </ul>
    </div>
    <div :class="cn('flex w-full flex-col p-2', collapsed && 'items-center')">
      <p v-if="!collapsed" class="flex h-8 items-center px-2 text-xs text-fg-3">Workspace</p>
      <ul class="flex w-full flex-col gap-0.5" :class="collapsed && 'items-center'">
        <li class="w-full" :class="collapsed && 'w-auto'">
          <NavLink
            :href="SETTINGS_NAV.general.href"
            :label="SETTINGS_NAV.general.label"
            :icon="SETTINGS_NAV.general.icon"
            :active="route.path === '/settings'"
            :collapsed="collapsed"
            @navigate="emit('navigate')"
          />
          <ul v-if="!collapsed && subItems.length" class="mx-3.5 mt-0.5 flex flex-col gap-0.5 border-l border-line py-0.5 pl-2.5">
            <li v-for="item in subItems" :key="item.href">
              <NavLink
                :href="item.href"
                :label="item.label"
                :active="isActive(route.path, item.href)"
                sub
                @navigate="emit('navigate')"
              />
            </li>
          </ul>
        </li>
        <template v-if="collapsed">
          <li v-for="item in subItems" :key="item.href">
            <NavLink
              :href="item.href"
              :label="item.label"
              :icon="item.icon"
              :active="isActive(route.path, item.href)"
              collapsed
              @navigate="emit('navigate')"
            />
          </li>
        </template>
      </ul>
    </div>
  </nav>
</template>
