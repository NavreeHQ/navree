"use client";

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { FormAlert } from "@/components/forms";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { errorMessage } from "@/lib/api/errors";
import { keys, useWorkspaceSlug } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

/**
 * The fake provider's checkout (development: `billing.provider = "fake"`). Paying completes it
 * as the real provider's webhooks would; with Stripe or Paddle, checkout happens on their pages.
 */
function FakeCheckout() {
  const slug = useWorkspaceSlug();
  const id = useSearchParams().get("id") ?? "";
  const router = useRouter();
  const queryClient = useQueryClient();
  const billing = workspacePath(slug, "/settings/billing");
  const pay = useMutation({
    mutationFn: () =>
      api.post(`/api/v1/settings/billing/fake-checkout/${encodeURIComponent(id)}`, undefined, { workspace: slug }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: keys.billing(slug) });
      router.replace(billing);
    },
    meta: { silent: true },
  });

  return (
    <>
      <PageHeader title="Checkout" description="A test payment: no card is charged." />
      <Card className="max-w-md">
        <CardHeader>
          <CardTitle>Fake payment provider</CardTitle>
          <CardDescription>
            Development only. Set <code>billing.provider</code> to <code>stripe</code> or <code>paddle</code> for real
            payments.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FormAlert message={pay.error ? errorMessage(pay.error) : undefined} />
        </CardContent>
        <CardFooter className="flex gap-2">
          <Button onClick={() => pay.mutate()} disabled={pay.isPending || !id}>
            {pay.isPending ? <Spinner data-icon="inline-start" /> : null}
            Pay
          </Button>
          <Button variant="ghost" onClick={() => router.replace(billing)}>
            Cancel
          </Button>
        </CardFooter>
      </Card>
    </>
  );
}

export default function FakeCheckoutPage() {
  return (
    <Suspense>
      <FakeCheckout />
    </Suspense>
  );
}
