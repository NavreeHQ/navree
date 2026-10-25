import { AuthenticatedApp } from "@/components/app-shell/authenticated-app";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  return <AuthenticatedApp>{children}</AuthenticatedApp>;
}
