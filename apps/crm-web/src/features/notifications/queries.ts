"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "@/lib/api/client";
import { keys } from "@/lib/api/keys";

export function useUnreadCount(enabled = true) {
  return useQuery({
    queryKey: keys.notifications.unreadCount,
    queryFn: () => unwrap(api.GET("/api/notifications/unread-count")),
    // Real-time pushes `notification.created`; polling is the fallback when the socket is down.
    refetchInterval: 60_000,
    enabled,
  });
}

export function useNotifications(enabled: boolean) {
  const params = { per_page: 12 };
  return useQuery({
    queryKey: keys.notifications.list(params),
    queryFn: () => unwrap(api.GET("/api/notifications", { params: { query: params } })),
    enabled,
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
