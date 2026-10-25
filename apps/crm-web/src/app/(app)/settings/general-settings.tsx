"use client";

import { useAbility } from "@casl/react";
import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { useTheme } from "next-themes";
import { roleLabel } from "@/components/app-shell/user-menu";
import { DetailRow, Panel } from "@/components/page-header";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UserAvatar } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import type { AppAbility, AppAction } from "@/lib/casl/ability";
import type { SubjectName } from "@/lib/api/types";
import { formatDateLong } from "@/lib/format";
import { env } from "@/lib/env";

const PERMISSION_ROWS: { subject: SubjectName; label: string }[] = [
  { subject: "Company", label: "Companies" },
  { subject: "Contact", label: "Contacts" },
  { subject: "Deal", label: "Deals" },
  { subject: "Activity", label: "Activities and tasks" },
  { subject: "User", label: "Users" },
];

/** Summarises what the current rules allow, so people know why a button is missing. */
function AccessSummary() {
  const ability = useAbility<AppAbility>();
  const describe = (subject: SubjectName): string => {
    const can = (action: AppAction) => ability.can(action, subject);
    const all = (action: AppAction) => ability.rulesFor(action, subject).some((r) => !r.inverted && !r.conditions);
    if (ability.can("manage", subject) && all("manage")) return "Full access";
    const parts: string[] = [];
    if (can("read")) parts.push("view");
    if (can("create")) parts.push("create");
    if (can("update")) parts.push(all("update") ? "edit any" : "edit your own");
    if (subject === "Contact" && can("export")) parts.push("export");
    if (subject === "Deal" && can("move")) parts.push(all("move") ? "move any" : "move your own");
    if (can("delete")) parts.push(all("delete") ? "delete any" : "delete your own");
    if (parts.length === 0) return "No access";
    const text = parts.join(", ");
    return text.charAt(0).toUpperCase() + text.slice(1);
  };
  return (
    <dl>
      {PERMISSION_ROWS.map((row) => (
        <DetailRow key={row.subject} label={row.label}>
          {describe(row.subject)}
        </DetailRow>
      ))}
    </dl>
  );
}

export function GeneralSettings() {
  const me = useCurrentUser();
  const { theme, setTheme } = useTheme();
  const user = me.data;
  if (!user) return null;

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <Panel title="Profile">
        <div className="flex items-center gap-3 pb-3">
          <UserAvatar name={user.name} id={user.id} size="md" />
          <div className="flex flex-col">
            <span className="font-medium">{user.name}</span>
            <span className="text-sm text-muted-foreground">{user.email}</span>
          </div>
        </div>
        <dl className="border-t pt-2">
          <DetailRow label="Role">{roleLabel(user.roles)}</DetailRow>
          <DetailRow label="Member since">{formatDateLong(user.created_at)}</DetailRow>
          <DetailRow label="Email verified">
            {user.email_verified_at ? formatDateLong(user.email_verified_at) : "Not yet"}
          </DetailRow>
        </dl>
        <p className="mt-3 text-xs text-muted-foreground">Ask an admin to change your name or role.</p>
      </Panel>
      <Panel title="Appearance">
        <div className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">Choose how Keel looks on this device.</p>
          <ToggleGroup
            type="single"
            variant="outline"
            value={theme ?? "system"}
            onValueChange={(v) => v && setTheme(v)}
            aria-label="Theme"
            className="self-start"
          >
            <ToggleGroupItem value="light" className="gap-1.5 px-3">
              <SunIcon />
              Light
            </ToggleGroupItem>
            <ToggleGroupItem value="dark" className="gap-1.5 px-3">
              <MoonIcon />
              Dark
            </ToggleGroupItem>
            <ToggleGroupItem value="system" className="gap-1.5 px-3">
              <MonitorIcon />
              Match system
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </Panel>
      <Panel title="What you can do">
        <AccessSummary />
        <p className="mt-3 text-xs text-muted-foreground">
          Permissions come from your role. Actions you can&apos;t take are hidden across the app.
        </p>
      </Panel>
      <Panel title="Connection">
        <dl>
          <DetailRow label="Data source">
            {env.mocks ? "Mock API (demo data in this browser tab)" : "Keel CRM API, through this app's server"}
          </DetailRow>
          <DetailRow label="Live updates">
            {env.mocks ? "Simulated in this tab" : env.realtime ? `WebSocket ${env.wsHost}:${env.wsPort}` : "Off"}
          </DetailRow>
        </dl>
      </Panel>
    </div>
  );
}
