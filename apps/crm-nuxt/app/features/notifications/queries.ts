import { useMutation, useQuery, useQueryClient } from "@tanstack/vue-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";
import type { AppNotification } from "@/lib/api/types";

export function useUnreadCount() {
  return useQuery({
    queryKey: keys.notifications.unreadCount,
    queryFn: () => unwrap(api.GET("/api/notifications/unread-count")),
    // Real time pushes `notification.created`; polling is the fallback when the socket is down.
    refetchInterval: 60_000,
  });
}

export function useNotifications(enabled: MaybeRefOrGetter<boolean>) {
  const params = { per_page: 12 };
  return useQuery({
    queryKey: keys.notifications.list(params),
    queryFn: () => unwrap(api.GET("/api/notifications", { params: { query: params } })),
    enabled: computed(() => toValue(enabled)),
  });
}

export function useMarkNotificationRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => unwrap(api.POST("/api/notifications/{id}/read", { params: { path: { id } } })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.notifications.all }),
  });
}

export function useMarkAllNotificationsRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(api.POST("/api/notifications/read-all")),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.notifications.all }),
  });
}

/**
 * Title and message for a notification. Navree sends `data: {title, message, url, deal_id?}`;
 * anything else falls back to the type.
 */
export function notificationParts(n: Pick<AppNotification, "type" | "data">): { title: string; message?: string } {
  const data = n.data ?? {};
  const text = (key: string) => (typeof data[key] === "string" && data[key] ? (data[key] as string) : undefined);
  const fallback = n.type.replace(/[._]/g, " ");
  const title = text("title");
  const message = text("message") ?? text("body");
  if (title) return { title, message };
  return { title: message ?? fallback.charAt(0).toUpperCase() + fallback.slice(1) };
}

export function notificationHref(n: AppNotification): string | null {
  const data = n.data ?? {};
  if (typeof data.url === "string" && data.url.startsWith("/")) return data.url;
  if (typeof data.deal_id === "string") return `/deals/${data.deal_id}`;
  if (typeof data.company_id === "string") return `/companies/${data.company_id}`;
  if (typeof data.contact_id === "string") return `/contacts/${data.contact_id}`;
  return null;
}
