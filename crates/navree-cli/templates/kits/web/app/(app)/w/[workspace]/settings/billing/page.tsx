"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api/client";
import type { BillingOverview, CheckoutUrl, DataList, Interval, Invoice, Plan } from "@/lib/api/types";
import { keys, useWorkspaceSlug } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

/** The fake provider's checkout pages (`billing.provider = "fake"`) open in the app instead. */
const FAKE_CHECKOUT = "https://checkout.fake.test/c/";

const money = (cents: number, currency: string) =>
  new Intl.NumberFormat(undefined, { style: "currency", currency: currency.toUpperCase() }).format(cents / 100);

const date = (value?: string | null) => (value ? new Date(value).toLocaleDateString() : "");

/**
 * The workspace's plan: checkout (with the plan's trial), changing plans, the provider's
 * customer portal, cancel and resume, and invoices. Seats follow the member count.
 */
export default function BillingPage() {
  const slug = useWorkspaceSlug();
  const router = useRouter();
  const queryClient = useQueryClient();
  const [period, setPeriod] = useState<Interval>("monthly");
  const overview = useQuery({
    queryKey: keys.billing(slug),
    queryFn: () => api.get<BillingOverview>("/api/billing", { workspace: slug }),
  });
  const invoices = useQuery({
    queryKey: keys.invoices(slug),
    queryFn: async () => (await api.get<DataList<Invoice>>("/api/billing/invoices", { workspace: slug })).data,
    enabled: Boolean(overview.data?.subscription),
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: keys.billing(slug) });

  const checkout = useMutation({
    mutationFn: (plan: Plan) => api.post<CheckoutUrl>("/api/billing/checkout", { plan: plan.key, interval: period }, { workspace: slug }),
    onSuccess: ({ url }) => {
      if (url.startsWith(FAKE_CHECKOUT)) {
        const id = url.slice(FAKE_CHECKOUT.length);
        router.push(workspacePath(slug, `/settings/billing/fake-checkout?id=${encodeURIComponent(id)}`));
      } else {
        window.location.assign(url);
      }
    },
  });
  const swap = useMutation({
    mutationFn: (plan: Plan) => api.post("/api/billing/swap", { plan: plan.key, interval: period }, { workspace: slug }),
    onSuccess: refresh,
  });
  const cancel = useMutation({
    mutationFn: () => api.post("/api/billing/cancel", undefined, { workspace: slug }),
    onSuccess: refresh,
  });
  const resume = useMutation({
    mutationFn: () => api.post("/api/billing/resume", undefined, { workspace: slug }),
    onSuccess: refresh,
  });
  const portal = useMutation({
    mutationFn: () =>
      api.post<CheckoutUrl>(
        "/api/billing/portal",
        { return_url: `${window.location.origin}${workspacePath(slug, "/settings/billing")}` },
        { workspace: slug },
      ),
    onSuccess: ({ url }) => window.location.assign(url),
  });

  if (overview.isPending) return <Skeleton className="h-64" />;
  const data = overview.data;
  if (!data) return null;
  const subscription = data.subscription;
  const currentPlan = data.plans.find((plan) => plan.key === subscription?.plan);

  return (
    <>
      <PageHeader title="Billing" description="The workspace's plan. Seats follow the number of members." />

      <Card>
        <CardHeader>
          <CardTitle>{subscription ? (currentPlan?.name ?? subscription.plan) : "No plan yet"}</CardTitle>
          <CardDescription>
            {subscription
              ? subscription.on_trial
                ? `Trial until ${date(subscription.trial_ends_at)}, ${subscription.quantity} seat${subscription.quantity === 1 ? "" : "s"}.`
                : subscription.canceled
                  ? `Canceled: ends ${date(subscription.ends_at)}.`
                  : `${subscription.interval === "yearly" ? "Yearly" : "Monthly"}, ${subscription.quantity} seat${subscription.quantity === 1 ? "" : "s"}, renews ${date(subscription.current_period_end)}.`
              : "Pick a plan below. Each comes with a free trial."}
          </CardDescription>
        </CardHeader>
        {subscription ? (
          <CardFooter className="flex flex-wrap gap-2">
            <Badge variant={subscription.valid ? "success" : "warning"}>{subscription.status.replace("_", " ")}</Badge>
            <Button variant="outline" onClick={() => portal.mutate()} disabled={portal.isPending}>
              Payment details and invoices
            </Button>
            {subscription.canceled && subscription.on_grace_period ? (
              <Button variant="outline" onClick={() => resume.mutate()} disabled={resume.isPending}>
                Resume
              </Button>
            ) : !subscription.canceled ? (
              <Button variant="ghost" onClick={() => cancel.mutate()} disabled={cancel.isPending}>
                Cancel at the end of the period
              </Button>
            ) : null}
          </CardFooter>
        ) : null}
      </Card>

      <div className="flex items-center justify-between gap-4">
        <h2 className="text-lg font-semibold text-fg">Plans</h2>
        <Tabs value={period} onValueChange={(value) => setPeriod(value as Interval)}>
          <TabsList>
            <TabsTrigger value="monthly">Monthly</TabsTrigger>
            <TabsTrigger value="yearly">Yearly</TabsTrigger>
          </TabsList>
        </Tabs>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        {data.plans.map((plan) => {
          const current = plan.key === subscription?.plan;
          const offered = plan.intervals.includes(period);
          return (
            <Card key={plan.key}>
              <CardHeader>
                <CardTitle>{plan.name}</CardTitle>
                <CardDescription>
                  {plan.trial_days > 0 ? `${plan.trial_days}-day free trial. ` : ""}
                  {plan.seats ? "Billed per member." : "One price for the workspace."}
                </CardDescription>
              </CardHeader>
              <CardFooter>
                {current ? (
                  <Badge variant="outline">Current plan</Badge>
                ) : subscription && !subscription.canceled ? (
                  <Button variant="outline" disabled={!offered || swap.isPending} onClick={() => swap.mutate(plan)}>
                    Switch to {plan.name}
                  </Button>
                ) : (
                  <Button disabled={!offered || checkout.isPending} onClick={() => checkout.mutate(plan)}>
                    {plan.trial_days > 0 ? "Start the trial" : `Choose ${plan.name}`}
                  </Button>
                )}
              </CardFooter>
            </Card>
          );
        })}
      </div>

      {invoices.data && invoices.data.length > 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>Invoices</CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Number</TableHead>
                  <TableHead>Date</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="text-right">Total</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {invoices.data.map((invoice) => (
                  <TableRow key={invoice.id}>
                    <TableCell>
                      {invoice.hosted_url ? (
                        <a className="record-link" href={invoice.hosted_url} target="_blank" rel="noreferrer">
                          {invoice.number}
                        </a>
                      ) : (
                        invoice.number
                      )}
                    </TableCell>
                    <TableCell className="figures">{date(invoice.created_at)}</TableCell>
                    <TableCell className="capitalize">{invoice.status}</TableCell>
                    <TableCell className="text-right figures">{money(invoice.total_cents, invoice.currency)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      ) : null}
    </>
  );
}
