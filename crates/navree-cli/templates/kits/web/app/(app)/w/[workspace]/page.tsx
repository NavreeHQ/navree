"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { api, query } from "@/lib/api/client";
import type { BillingOverview, DataList, Member, ProjectPage } from "@/lib/api/types";
import { keys, useMe, useWorkspaceSlug, useWorkspaces } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

function Figure({ label, value, href }: { label: string; value?: string | number; href: string }) {
  return (
    <Card className="gap-2">
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        <CardTitle className="text-figure figures">
          {value === undefined ? <Skeleton className="h-7 w-16" /> : <Link href={href}>{value}</Link>}
        </CardTitle>
      </CardHeader>
    </Card>
  );
}

/** The workspace's dashboard: its projects, its team and its plan at a glance. */
export default function DashboardPage() {
  const slug = useWorkspaceSlug();
  const me = useMe();
  const current = useWorkspaces().data?.find((workspace) => workspace.slug === slug);
  const manager = current?.role === "owner" || current?.role === "admin";

  const active = useQuery({
    queryKey: [...keys.projects(slug), "count", "active"],
    queryFn: () => api.get<ProjectPage>(`/api/v1/projects${query({ status: "active", per_page: 1 })}`, { workspace: slug }),
  });
  const archived = useQuery({
    queryKey: [...keys.projects(slug), "count", "archived"],
    queryFn: () => api.get<ProjectPage>(`/api/v1/projects${query({ status: "archived", per_page: 1 })}`, { workspace: slug }),
  });
  const members = useQuery({
    queryKey: keys.members(slug),
    queryFn: async () => (await api.get<DataList<Member>>(`/api/workspaces/${slug}/members`, { workspace: slug })).data,
  });
  const billing = useQuery({
    queryKey: keys.billing(slug),
    queryFn: () => api.get<BillingOverview>("/api/billing", { workspace: slug }),
    enabled: manager,
  });
  const plan = billing.data?.subscription
    ? `${billing.data.plans.find((p) => p.key === billing.data?.subscription?.plan)?.name ?? billing.data.subscription.plan} (${billing.data.subscription.status.replace("_", " ")})`
    : billing.data
      ? "No plan yet"
      : undefined;

  return (
    <>
      <PageHeader
        title={`Welcome, ${me.data?.name.split(" ")[0] ?? ""}`}
        description={current ? `${current.name}: you are ${current.role === "owner" ? "the owner" : `a${current.role === "admin" ? "n" : ""} ${current.role}`}.` : undefined}
        actions={
          <Button asChild>
            <Link href={workspacePath(slug, "/projects")}>Open projects</Link>
          </Button>
        }
      />
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Figure label="Active projects" value={active.data?.meta.total} href={workspacePath(slug, "/projects?status=active")} />
        <Figure label="Archived projects" value={archived.data?.meta.total} href={workspacePath(slug, "/projects?status=archived")} />
        <Figure label="Members" value={members.data?.length} href={workspacePath(slug, "/settings/members")} />
        {manager ? <Figure label="Plan" value={plan} href={workspacePath(slug, "/settings/billing")} /> : null}
      </div>
    </>
  );
}
