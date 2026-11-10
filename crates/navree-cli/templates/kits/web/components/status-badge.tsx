import { Badge } from "@/components/ui/badge";

/** A project's status, in words. */
export function StatusBadge({ status }: { status: string }) {
  return (
    <Badge variant={status === "active" ? "secondary" : "outline"}>{status === "active" ? "Active" : "Archived"}</Badge>
  );
}
