<script setup lang="ts">
import { isNavreeError } from "@navree/client";
import ApiUnavailable from "@/components/shell/ApiUnavailable.vue";
import AppHeader from "@/components/shell/AppHeader.vue";
import AppSidebar from "@/components/shell/AppSidebar.vue";
import CommandPalette from "@/components/shell/CommandPalette.vue";
import CreateDialogs from "@/components/shell/CreateDialogs.vue";
import LiveConnection from "@/components/shell/LiveConnection.vue";
import MobileNav from "@/components/shell/MobileNav.vue";
import ShellSkeleton from "@/components/shell/ShellSkeleton.vue";
import ShortcutsDialog from "@/components/shell/ShortcutsDialog.vue";
import { useCurrentUser } from "@/features/auth/queries";

/**
 * Everything behind sign-in: loads the user (a 401 goes to /login through the
 * API client), connects real time and renders the shell. The user's CASL rules
 * come from the `@navree/nuxt` module, which loads them once the user is signed
 * in; the shell waits for them (`useAbilityReady`) so no screen or button is drawn
 * from an empty rule set and then changes.
 */
const user = useCurrentUser();
const abilitiesReady = useAbilityReady();

const unauthenticated = computed(() => isNavreeError(user.error.value) && user.error.value.status === 401);
</script>

<template>
  <a href="#content" class="skip-link">Skip to content</a>
  <ApiUnavailable v-if="user.isError.value && !unauthenticated" @retry="user.refetch()" />
  <ShellSkeleton v-else-if="!user.data.value || !abilitiesReady" />
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
