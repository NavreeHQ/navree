"use client";

import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/lib/api/client";
import { errorMessage } from "@/lib/api/errors";
import type { Workspace } from "@/lib/api/types";
import { nextQuery } from "@/lib/paths";
import { keys, useMe } from "@/lib/queries";
import { workspacePath } from "@/lib/workspace";

/** The page the invitation mail links to (`workspace.invitations.url`): `?token=..`. */
function AcceptInvitation() {
  const router = useRouter();
  const params = useSearchParams();
  const token = params.get("token") ?? "";
  const here = `/invitations/accept?token=${encodeURIComponent(token)}`;
  const me = useMe();
  const queryClient = useQueryClient();
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string>();

  async function accept() {
    setPending(true);
    try {
      const accepted = await api.post<{ workspace: Workspace }>(
        "/api/invitations/accept",
        { token },
        { workspace: null },
      );
      await queryClient.invalidateQueries({ queryKey: keys.workspaces });
      router.replace(workspacePath(accepted.workspace.slug));
    } catch (error) {
      setFailure(errorMessage(error));
      setPending(false);
    }
  }

  if (me.isPending) return null;
  if (!me.data) {
    return (
      <AuthShell title="You're invited" description="Sign in or create an account with the address the invitation was sent to.">
        <div className="flex flex-col gap-2">
          <Button size="lg" asChild>
            <Link href={`/sign-in${nextQuery(here)}`}>Sign in</Link>
          </Button>
          <Button variant="outline" size="lg" asChild>
            <Link href={`/sign-up${nextQuery(here)}`}>Create an account</Link>
          </Button>
        </div>
      </AuthShell>
    );
  }

  return (
    <AuthShell title="Join the workspace" description={`You're signed in as ${me.data.email}.`}>
      <FormAlert message={failure ?? (token ? undefined : "This link is incomplete.")} />
      <Button size="lg" onClick={accept} disabled={pending || !token}>
        {pending ? <Spinner data-icon="inline-start" /> : null}
        Accept the invitation
      </Button>
    </AuthShell>
  );
}

export default function AcceptInvitationPage() {
  return (
    <Suspense>
      <AcceptInvitation />
    </Suspense>
  );
}
