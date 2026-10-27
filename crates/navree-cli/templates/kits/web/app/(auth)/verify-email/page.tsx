"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";
import { AuthShell } from "@/components/auth-shell";
import { FormAlert } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api/client";
import { errorMessage } from "@/lib/api/errors";
import { safeNext } from "@/lib/paths";
import { useMe } from "@/lib/queries";

/**
 * After sign-up ("check your inbox"), and where the mailed link lands
 * (`?status=verified` or `invalid`).
 */
function VerifyEmail() {
  const params = useSearchParams();
  const status = params.get("status");
  const next = safeNext(params.get("next"));
  const me = useMe();
  const [notice, setNotice] = useState<string>();
  const [failure, setFailure] = useState<string>();

  async function resend() {
    try {
      await api.post("/api/auth/email/verification", undefined, { workspace: null });
      setNotice("We sent you a new link.");
    } catch (error) {
      setFailure(errorMessage(error));
    }
  }

  if (status === "verified") {
    return (
      <AuthShell title="Your email address is verified" description="Thanks for confirming it.">
        <Button size="lg" asChild>
          <Link href={next}>Continue</Link>
        </Button>
      </AuthShell>
    );
  }

  return (
    <AuthShell
      title={status === "invalid" ? "That link didn't work" : "Check your inbox"}
      description={
        status === "invalid"
          ? "It may have expired or been used already. Ask for a new one."
          : `We sent a link to ${me.data?.email ?? "your email address"}. Open it to verify your address.`
      }
    >
      <FormAlert message={failure} />
      <FormAlert message={notice} variant="default" />
      <div className="flex flex-col gap-2">
        {me.data ? (
          <Button variant="outline" size="lg" onClick={resend}>
            Send a new link
          </Button>
        ) : null}
        <Button size="lg" asChild>
          <Link href={me.data ? next : "/sign-in"}>{me.data ? "Continue" : "Sign in"}</Link>
        </Button>
      </div>
    </AuthShell>
  );
}

export default function VerifyEmailPage() {
  return (
    <Suspense>
      <VerifyEmail />
    </Suspense>
  );
}
