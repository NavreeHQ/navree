import type { AppNotification, Deal } from "@/lib/api/types";

/** `private-crm.deals` → `deal.updated` */
export type DealUpdatedPayload = { deal: Deal };

/** Notification `data`. */
export type NotificationData = { title?: string; message?: string; url?: string; deal_id?: string };

/**
 * `private-users.{id}` → `notification.created`: the notification's `data`
 * object. A full notification (`{id, type, data, ...}`) is accepted too.
 */
export type NotificationCreatedPayload = NotificationData | Partial<AppNotification>;

/** Channel names without the protocol's `private-` prefix (the pusher-js wrapper adds it). */
export const DEALS_CHANNEL = "crm.deals";
export const userChannel = (userId: string) => `users.${userId}`;
