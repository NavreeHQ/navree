import { cookies } from "next/headers";
import { AuthenticatedApp } from "@/components/app-shell/authenticated-app";

/** The sidebar's open/collapsed state, saved by shadcn's SidebarProvider (`sidebar_state`). */
const SIDEBAR_COOKIE = "sidebar_state";

export default async function AppLayout({ children }: { children: React.ReactNode }) {
  const sidebarOpen = (await cookies()).get(SIDEBAR_COOKIE)?.value !== "false";
  return <AuthenticatedApp sidebarOpen={sidebarOpen}>{children}</AuthenticatedApp>;
}
