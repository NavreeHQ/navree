"use client";

import { useRouter } from "next/navigation";
import { useEffect } from "react";
import { Spinner } from "@/components/ui/spinner";
import { useMe, useWorkspaces } from "@/lib/queries";
import { lastWorkspace, workspacePath } from "@/lib/workspace";

/** `/`: the workspace opened last (or the first), else sign-in or onboarding. */
export default function Home() {
  const router = useRouter();
  const me = useMe();
  const workspaces = useWorkspaces();

  useEffect(() => {
    if (me.isPending) return;
    if (!me.data) {
      router.replace("/sign-in");
      return;
    }
    if (!workspaces.data) return;
    if (workspaces.data.length === 0) {
      router.replace("/onboarding");
      return;
    }
    const last = lastWorkspace();
    const slug = workspaces.data.find((w) => w.slug === last)?.slug ?? workspaces.data[0].slug;
    router.replace(workspacePath(slug));
  }, [me.isPending, me.data, workspaces.data, router]);

  return (
    <main id="content" className="flex min-h-svh items-center justify-center" aria-busy="true">
      <Spinner className="size-5 text-fg-3" />
    </main>
  );
}
