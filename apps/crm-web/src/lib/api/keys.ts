/** TanStack Query keys. Invalidate by prefix (e.g. `keys.deals.all`). */
export const keys = {
  auth: {
    user: ["auth", "user"] as const,
    abilities: ["auth", "abilities"] as const,
  },
  users: {
    all: ["users"] as const,
    list: (params: object) => ["users", "list", params] as const,
  },
  companies: {
    all: ["companies"] as const,
    list: (params: object) => ["companies", "list", params] as const,
    detail: (id: string) => ["companies", "detail", id] as const,
  },
  contacts: {
    all: ["contacts"] as const,
    list: (params: object) => ["contacts", "list", params] as const,
    detail: (id: string) => ["contacts", "detail", id] as const,
  },
  deals: {
    all: ["deals"] as const,
    list: (params: object) => ["deals", "list", params] as const,
    board: (params: object) => ["deals", "board", params] as const,
    detail: (id: string) => ["deals", "detail", id] as const,
  },
  activities: {
    all: ["activities"] as const,
    list: (params: object) => ["activities", "list", params] as const,
  },
  dashboard: ["dashboard"] as const,
  notifications: {
    all: ["notifications"] as const,
    unreadCount: ["notifications", "unread-count"] as const,
    list: (params: object) => ["notifications", "list", params] as const,
  },
  audit: {
    all: ["audit"] as const,
    list: (params: object) => ["audit", "list", params] as const,
  },
};
