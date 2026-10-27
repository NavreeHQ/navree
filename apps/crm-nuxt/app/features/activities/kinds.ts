import { CalendarDays, Mail, Phone, SquareCheck, StickyNote } from "@lucide/vue";
import type { Component } from "vue";
import type { ActivityKind, ActivitySubjectType } from "@/lib/api/types";

export const KIND_META: Record<ActivityKind, { label: string; icon: Component }> = {
  note: { label: "Note", icon: StickyNote },
  call: { label: "Call", icon: Phone },
  email: { label: "Email", icon: Mail },
  meeting: { label: "Meeting", icon: CalendarDays },
  task: { label: "Task", icon: SquareCheck },
};

export function subjectHref(type: ActivitySubjectType, id: string): string {
  return type === "company" ? `/companies/${id}` : type === "contact" ? `/contacts/${id}` : `/deals/${id}`;
}
