import type { ContactStatus } from "@/lib/api/types";

export const STATUS_LABEL: Record<ContactStatus, string> = {
  lead: "Lead",
  prospect: "Prospect",
  customer: "Customer",
  churned: "Churned",
};
