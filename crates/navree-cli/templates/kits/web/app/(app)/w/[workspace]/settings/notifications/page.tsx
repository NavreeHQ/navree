"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageHeader } from "@/components/page-header";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Skeleton } from "@/components/ui/skeleton";
import { api } from "@/lib/api/client";
import type { NotificationSettings } from "@/lib/api/types";
import { keys, useWorkspaceSlug } from "@/lib/queries";

const OPTIONS: { key: keyof NotificationSettings; label: string; description: string }[] = [
  { key: "project_activity", label: "Project activity", description: "An email when a project is created or archived." },
  { key: "weekly_summary", label: "Weekly summary", description: "What happened in the workspace, every Monday." },
  { key: "product_updates", label: "Product updates", description: "News about the product, now and then." },
];

/** What the signed-in person hears about from this workspace (their own choice, per workspace). */
export default function NotificationsPage() {
  const slug = useWorkspaceSlug();
  const queryClient = useQueryClient();
  const settings = useQuery({
    queryKey: keys.notifications(slug),
    queryFn: () => api.get<NotificationSettings>("/api/v1/settings/notifications", { workspace: slug }),
  });
  const save = useMutation({
    mutationFn: (change: Partial<NotificationSettings>) =>
      api.put<NotificationSettings>("/api/v1/settings/notifications", change, { workspace: slug }),
    onSuccess: (saved) => queryClient.setQueryData(keys.notifications(slug), saved),
  });

  return (
    <>
      <PageHeader title="Notifications" description="Your choices for this workspace." />
      <Card>
        <CardContent>
          {settings.data ? (
            <FieldGroup>
              {OPTIONS.map((option) => (
                <Field key={option.key} orientation="horizontal">
                  <Checkbox
                    id={option.key}
                    checked={settings.data[option.key]}
                    disabled={save.isPending}
                    onCheckedChange={(checked) => save.mutate({ [option.key]: checked === true })}
                  />
                  <FieldContent>
                    <FieldLabel htmlFor={option.key}>{option.label}</FieldLabel>
                    <FieldDescription>{option.description}</FieldDescription>
                  </FieldContent>
                </Field>
              ))}
            </FieldGroup>
          ) : (
            <Skeleton className="h-32" />
          )}
        </CardContent>
      </Card>
    </>
  );
}
