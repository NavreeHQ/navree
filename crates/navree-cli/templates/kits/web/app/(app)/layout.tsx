"use client";

import { usePathname, useRouter } from "next/navigation";
import { useEffect, type ReactNode } from "react";
import { Spinner } from "@/components/ui/spinner";
import { useMe } from "@/lib/queries";

/** Everything behind sign-in: guests go to the sign-in page and come back. */
export default function SignedInLayout({ children }: { children: ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const me = useMe();

  useEffect(() => {
    if (me.data === null) router.replace(`/sign-in?next=${encodeURIComponent(pathname)}`);
  }, [me.data, pathname, router]);

  if (!me.data) {
    return (
      <main id="content" className="flex min-h-svh items-center justify-center" aria-busy="true">
        <Spinner className="size-5 text-fg-3" />
      </main>
    );
  }
  return children;
}
