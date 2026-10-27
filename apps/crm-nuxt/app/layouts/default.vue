<script setup lang="ts">
import { useAbility } from "@casl/vue";
import ApiUnavailable from "@/components/shell/ApiUnavailable.vue";
import AppHeader from "@/components/shell/AppHeader.vue";
import AppSidebar from "@/components/shell/AppSidebar.vue";
import CommandPalette from "@/components/shell/CommandPalette.vue";
import CreateDialogs from "@/components/shell/CreateDialogs.vue";
import LiveConnection from "@/components/shell/LiveConnection.vue";
import MobileNav from "@/components/shell/MobileNav.vue";
import ShellSkeleton from "@/components/shell/ShellSkeleton.vue";
import ShortcutsDialog from "@/components/shell/ShortcutsDialog.vue";
import { useAbilityRules, useCurrentUser } from "@/features/auth/queries";
import { isApiError } from "@/lib/api/errors";
import type { AppAbility } from "@/lib/casl/ability";

/**
 * Everything behind sign-in: loads the user (a 401 goes to /login through the
 * API client), applies their CASL rules to the app-wide ability before any
 * screen renders, connects real time and renders the shell.
 */
const user = useCurrentUser();
const rules = useAbilityRules(() => Boolean(user.data.value));
const ability = useAbility<AppAbility>();
const applied = ref<unknown>(null);

watch(
  () => rules.data.value,
  (next) => {
    if (!next || applied.value === next) return;
    ability.update(next as Parameters<AppAbility["update"]>[0]);
    applied.value = next;
  },
  { immediate: true },
);

const unauthenticated = computed(() => isApiError(user.error.value) && user.error.value.status === 401);
const ready = computed(() => Boolean(user.data.value) && rules.data.value !== undefined && applied.value === rules.data.value);
</script>

<template>
  <a href="#content" class="skip-link">Skip to content</a>
  <ApiUnavailable v-if="user.isError.value && !unauthenticated" @retry="user.refetch()" />
  <ApiUnavailable
    v-else-if="rules.isError.value"
    detail="Your permissions couldn't be loaded."
    @retry="rules.refetch()"
  />
  <ShellSkeleton v-else-if="!ready || !user.data.value" />
  <div v-else class="flex min-h-dvh">
    <AppSidebar :user="user.data.value" />
    <div class="flex min-w-0 flex-1 flex-col">
      <AppHeader />
      <main id="content" tabindex="-1" class="flex min-w-0 flex-1 flex-col px-4 pt-5 pb-10 outline-none sm:px-6 sm:pt-6">
        <slot />
      </main>
    </div>
    <MobileNav :user="user.data.value" />
    <CommandPalette />
    <ShortcutsDialog />
    <CreateDialogs />
    <LiveConnection :user-id="user.data.value.id" />
  </div>
</template>
