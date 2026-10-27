"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { Spinner } from "@/components/ui/spinner";
import { lastWorkspace, workspacePath } from "@/lib/workspace";

/**
 * Where checkout returns (`billing.return_url`): back to the billing settings of the
 * workspace the person was in.
 */
export default function BillingReturnPage() {
  const router = useRouter();
  useEffect(() => {
    const slug = lastWorkspace();
    router.replace(slug ? workspacePath(slug, "/settings/billing") : "/");
  }, [router]);
  return (
    <main id="content" className="flex min-h-svh items-center justify-center" aria-busy="true">
      <Spinner className="size-5 text-fg-3" />
    </main>
  );
}
