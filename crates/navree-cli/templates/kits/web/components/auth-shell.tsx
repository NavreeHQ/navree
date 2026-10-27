import type { ReactNode } from "react";
import { APP_NAME } from "@/lib/app";

/** The frame of the sign-in, sign-up and account-recovery screens. */
export function AuthShell({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <main id="content" className="flex min-h-svh items-center justify-center px-4 py-12">
      <div className="flex w-full max-w-sm flex-col gap-8">
        <p className="font-display text-lg font-semibold text-fg">{APP_NAME}</p>
        <div className="flex flex-col gap-1.5">
          <h1 className="text-title font-semibold text-fg">{title}</h1>
          {description ? <p className="text-sm text-fg-2">{description}</p> : null}
        </div>
        {children}
      </div>
    </main>
  );
}
