<script setup lang="ts">
import { Bell, CheckCheck } from "@lucide/vue";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import {
  notificationHref,
  notificationParts,
  useMarkAllNotificationsRead,
  useMarkNotificationRead,
  useNotifications,
  useUnreadCount,
} from "@/features/notifications/queries";
import type { AppNotification } from "@/lib/api/types";
import { formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";

/** Deal assignments, wins and task reminders, with the unread count on the bell. */
const open = ref(false);
const unread = useUnreadCount();
const list = useNotifications(open);
const markRead = useMarkNotificationRead();
const markAll = useMarkAllNotificationsRead();
const count = computed(() => unread.data.value?.count ?? 0);

function openNotification(n: AppNotification) {
  if (!n.read_at) markRead.mutate(n.id);
  const href = notificationHref(n);
  open.value = false;
  if (href) void navigateTo(href);
}
</script>

<template>
  <Popover v-model:open="open">
    <PopoverTrigger as-child>
      <Button
        variant="ghost"
        size="icon-sm"
        class="relative"
        :aria-label="count > 0 ? `Notifications, ${count} unread` : 'Notifications'"
      >
        <Bell />
        <span
          v-if="count > 0"
          data-testid="unread-count"
          class="absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-sm bg-fg px-1 font-mono text-xs leading-none font-semibold text-ink-0 ring-2 ring-ink-0 figures"
          >{{ count > 99 ? "99+" : count }}</span
        >
      </Button>
    </PopoverTrigger>
    <PopoverContent align="end" :collision-padding="12" class="w-[min(24rem,calc(100vw-1.5rem))] p-0">
      <div class="flex items-center justify-between border-b border-line px-4 py-2.5">
        <h2 class="text-sm font-semibold text-fg">Notifications</h2>
        <Button variant="ghost" size="xs" :disabled="count === 0 || markAll.isPending.value" @click="markAll.mutate()">
          <CheckCheck />
          Mark all read
        </Button>
      </div>
      <div class="max-h-96 overflow-y-auto scroll-thin">
        <div v-if="list.isPending.value" class="flex flex-col gap-3 p-4">
          <Skeleton class="h-4 w-3/4" />
          <Skeleton class="h-4 w-2/3" />
          <Skeleton class="h-4 w-1/2" />
        </div>
        <ul v-else-if="list.data.value && list.data.value.data.length > 0" class="divide-y divide-line">
          <li v-for="n in list.data.value.data" :key="n.id">
            <button
              type="button"
              class="flex w-full items-start gap-3 px-4 py-3 text-left text-sm transition-colors hover:bg-ink-2 focus-visible:bg-ink-2"
              @click="openNotification(n)"
            >
              <span
                aria-hidden="true"
                :class="cn('mt-1.5 size-1.5 shrink-0 rounded-full', n.read_at ? 'bg-transparent' : 'bg-accent')"
              />
              <span class="flex min-w-0 flex-col gap-0.5">
                <span :class="cn('leading-snug text-fg-1', !n.read_at && 'font-medium text-fg')">{{ notificationParts(n).title }}</span>
                <span v-if="notificationParts(n).message" class="text-[13px] leading-snug text-fg-2">{{
                  notificationParts(n).message
                }}</span>
                <span class="text-xs text-fg-3">
                  {{ formatRelative(n.created_at) }}<span v-if="!n.read_at" class="sr-only">, unread</span>
                </span>
              </span>
            </button>
          </li>
        </ul>
        <p v-else class="px-6 py-10 text-center text-[13px] leading-relaxed text-fg-3">
          Nothing new. Deal assignments, wins and task reminders show up here.
        </p>
      </div>
    </PopoverContent>
  </Popover>
</template>
