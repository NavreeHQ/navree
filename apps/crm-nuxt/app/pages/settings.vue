<script setup lang="ts">
import { useAbility } from "@casl/vue";
import { useCurrentUser } from "@/features/auth/queries";
import type { AppAbility } from "@/lib/casl/ability";
import { cn } from "@/lib/utils";

const route = useRoute();
const ability = useAbility<AppAbility>();
const me = useCurrentUser();
const items = computed(() =>
  [
    { href: "/settings", label: "General", show: true },
    { href: "/settings/users", label: "Users and roles", show: ability.can("create", "User") },
    { href: "/settings/audit", label: "Audit log", show: Boolean(me.data.value?.roles.includes("admin")) },
  ].filter((i) => i.show),
);
</script>

<template>
  <div class="flex flex-col">
    <header class="flex flex-col gap-4 pb-6">
      <div class="flex flex-col gap-1">
        <h1 class="font-sans text-2xl leading-8 font-semibold tracking-[-0.02em] text-fg">Settings</h1>
        <p class="text-sm text-fg-2">Your account, your team and what changed in the workspace.</p>
      </div>
      <nav aria-label="Settings sections" class="-mx-4 flex gap-1 overflow-x-auto border-b border-line px-4 sm:mx-0 sm:px-0">
        <NuxtLink
          v-for="item in items"
          :key="item.href"
          :to="item.href"
          :aria-current="route.path === item.href ? 'page' : undefined"
          :class="
            cn(
              '-mb-px border-b-2 px-3 py-2.5 text-sm whitespace-nowrap transition-colors',
              route.path === item.href ? 'border-fg font-medium text-fg' : 'border-transparent text-fg-3 hover:text-fg-1',
            )
          "
          >{{ item.label }}</NuxtLink
        >
      </nav>
    </header>
    <NuxtPage />
  </div>
</template>
