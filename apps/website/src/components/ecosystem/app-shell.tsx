import { CheckIcon, ChevronRightIcon, ChevronsUpDownIcon, PanelLeftIcon, SearchIcon, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/*
 * The app shell every product mock on the site is drawn with: the shadcn/ui
 * Sidebar pattern (the `sidebar-07` layout) in Navree's ink, as DESIGN.md
 * section 9 describes it. A 16rem sidebar with a workspace or app switcher,
 * quiet group labels, 32px items with 16px lucide icons, ink-based active
 * states and a user footer; it collapses to a 3rem icon rail on medium
 * frames and leaves the frame (an off-canvas sheet) on phones. The inset
 * has a 48px top bar: trigger, separator, breadcrumb, search and actions.
 *
 * Corners: 4px on controls, 6px on cards, popovers and dialogs. Green only
 * for the primary action and positive data.
 */

export type NavItem = {
  icon: LucideIcon;
  label: string;
  /** A count or short badge, right-aligned in mono. */
  badge?: string;
  active?: boolean;
  /** Sub-items, indented on a 1px guide. */
  sub?: { label: string; active?: boolean }[];
};
export type NavGroup = { label?: string; items: NavItem[] };
export type SidebarSpec = {
  /** The workspace or app in the switcher: a logo mark, its name and its plan or context. */
  app: { logo: ReactNode; name: string; context: string };
  groups: NavGroup[];
  user: { initials: string; name: string; email: string };
};

/** The 32px logo tile of the switcher (24px in menus). */
export function LogoTile({ children, size = "md", tone = "solid", className }: { children: ReactNode; size?: "sm" | "md"; tone?: "solid" | "outline"; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-sm",
        size === "md" ? "size-8 [&>svg]:size-4" : "size-6 [&>svg]:size-3.5",
        tone === "solid" ? "bg-fg text-ink-0" : "border border-line-2 bg-ink-1 text-fg-1",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** A user's avatar: a 4px square with their initials (32px in the footer, 24px in tables). */
export function Avatar({ initials, size = "md", className }: { initials: string; size?: "sm" | "md"; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-sm border border-line-2 bg-ink-3 font-medium text-fg-1",
        size === "md" ? "size-8 text-xs" : "size-6 text-[0.6875rem]",
        className,
      )}
    >
      {initials}
    </span>
  );
}

function Switcher({ app, collapsed }: { app: SidebarSpec["app"]; collapsed?: boolean }) {
  if (collapsed) return <LogoTile>{app.logo}</LogoTile>;
  return (
    <div className="flex h-12 items-center gap-2 rounded-sm p-2">
      <LogoTile>{app.logo}</LogoTile>
      <div className="grid min-w-0 flex-1 leading-tight">
        <span className="truncate text-sm font-medium text-fg">{app.name}</span>
        <span className="truncate text-xs text-fg-3">{app.context}</span>
      </div>
      <ChevronsUpDownIcon className="size-4 shrink-0 text-fg-3" strokeWidth={1.75} />
    </div>
  );
}

function UserButton({ user, collapsed }: { user: SidebarSpec["user"]; collapsed?: boolean }) {
  if (collapsed) return <Avatar initials={user.initials} />;
  return (
    <div className="flex h-12 items-center gap-2 rounded-sm p-2">
      <Avatar initials={user.initials} />
      <div className="grid min-w-0 flex-1 leading-tight">
        <span className="truncate text-sm font-medium text-fg">{user.name}</span>
        <span className="truncate text-xs text-fg-3">{user.email}</span>
      </div>
      <ChevronsUpDownIcon className="size-4 shrink-0 text-fg-3" strokeWidth={1.75} />
    </div>
  );
}

/** One sidebar item: 32px, a 16px icon, 14px text; ink when active. */
export function SidebarItem({ item, collapsed }: { item: NavItem; collapsed?: boolean }) {
  const Icon = item.icon;
  if (collapsed) {
    return (
      <span className={cn("flex size-8 items-center justify-center rounded-sm", item.active ? "bg-ink-3 text-fg" : "text-fg-2")}>
        <Icon className="size-4" strokeWidth={1.75} />
      </span>
    );
  }
  return (
    <div>
      {/* Baseline-aligned (the mono count sits on the label's baseline, not a box centre), padded to 32px. */}
      <div className={cn("flex h-8 items-baseline gap-2 rounded-sm px-2 py-1.5 text-sm leading-5", item.active ? "bg-ink-3 font-medium text-fg" : "text-fg-2")}>
        <Icon className="size-4 shrink-0 self-center" strokeWidth={1.75} />
        <span className="min-w-0 flex-1 truncate">{item.label}</span>
        {item.badge ? <span className="font-mono text-xs font-normal text-fg-3 tabular-nums">{item.badge}</span> : null}
        {item.sub ? <ChevronRightIcon className="size-4 shrink-0 rotate-90 self-center text-fg-3" strokeWidth={1.75} /> : null}
      </div>
      {item.sub ? (
        <div className="mx-3.5 flex flex-col gap-1 border-l border-line px-2.5 py-0.5">
          {item.sub.map((s) => (
            <span key={s.label} className={cn("flex h-7 items-center rounded-sm px-2 text-sm", s.active ? "bg-ink-3 font-medium text-fg" : "text-fg-2")}>
              <span className="truncate">{s.label}</span>
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** The sidebar's contents, expanded (16rem) or as the icon rail (3rem). */
export function SidebarBody({ spec, collapsed = false, className }: { spec: SidebarSpec; collapsed?: boolean; className?: string }) {
  return (
    <div className={cn("flex h-full min-h-0 flex-col", className)}>
      <div className="p-2">
        <Switcher app={spec.app} collapsed={collapsed} />
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {spec.groups.map((group, i) => (
          <div key={group.label ?? i} className="flex flex-col p-2">
            {group.label && !collapsed ? <div className="flex h-8 shrink-0 items-center px-2 text-xs text-fg-3">{group.label}</div> : null}
            <div className="flex flex-col gap-1">
              {group.items.map((item) => (
                <SidebarItem key={item.label} item={item} collapsed={collapsed} />
              ))}
            </div>
          </div>
        ))}
      </div>
      <div className="p-2">
        <UserButton user={spec.user} collapsed={collapsed} />
      </div>
    </div>
  );
}

/**
 * When the sidebar shows, by the width of the frame (container queries):
 * `auto` is gone on phones, a rail from 42rem, expanded from 64rem; `rail`
 * is a rail from 32rem; `full` is expanded from 48rem.
 */
const modes = {
  auto: {
    grid: "@2xl:grid-cols-[3rem_minmax(0,1fr)] @5xl:grid-cols-[16rem_minmax(0,1fr)]",
    full: "hidden @5xl:flex",
    rail: "hidden @2xl:flex @5xl:hidden",
  },
  rail: {
    grid: "@lg:grid-cols-[3rem_minmax(0,1fr)]",
    full: "hidden",
    rail: "hidden @lg:flex",
  },
  full: {
    grid: "@3xl:grid-cols-[16rem_minmax(0,1fr)]",
    full: "hidden @3xl:flex",
    rail: "hidden",
  },
} as const;

/** The whole shell: sidebar and inset. Put it in a `@container` (MockFrame's body is one). */
export function AppShell({
  sidebar,
  mode = "auto",
  children,
  overlay,
  className,
}: {
  sidebar: SidebarSpec;
  mode?: keyof typeof modes;
  children: ReactNode;
  /** Drawn over the whole shell: a dialog, a toast. */
  overlay?: ReactNode;
  className?: string;
}) {
  const m = modes[mode];
  return (
    <div className={cn("relative grid h-full min-h-full bg-ink-0", m.grid, className)}>
      <aside className={cn("min-h-0 flex-col border-r border-line bg-ink-1", m.full)}>
        <SidebarBody spec={sidebar} />
      </aside>
      <aside className={cn("min-h-0 flex-col items-center border-r border-line bg-ink-1", m.rail)}>
        <SidebarBody spec={sidebar} collapsed className="items-center" />
      </aside>
      {/* The inset is a container of its own: what's inside lays out by the inset's width, not the frame's. */}
      <div className="@container relative flex min-w-0 flex-col">{children}</div>
      {overlay}
    </div>
  );
}

/** The sidebar trigger: a ghost icon button. */
export function SidebarTrigger({ className }: { className?: string }) {
  return (
    <span className={cn("-ml-1 inline-flex size-7 shrink-0 items-center justify-center rounded-sm text-fg-2", className)}>
      <PanelLeftIcon className="size-4" strokeWidth={1.75} />
    </span>
  );
}

/** The breadcrumb: ancestors in fg-3 (hidden on narrow frames), the page in fg. */
export function Breadcrumb({ items, className }: { items: string[]; className?: string }) {
  return (
    <div className={cn("flex min-w-0 items-center gap-1.5 text-sm", className)}>
      {items.map((item, i) => {
        const last = i === items.length - 1;
        return (
          <span key={item} className={cn("min-w-0 items-center gap-1.5", last ? "flex" : "hidden @lg:flex")}>
            <span className={cn("truncate", last ? "text-fg" : "text-fg-3")}>{item}</span>
            {last ? null : <ChevronRightIcon className="size-3.5 shrink-0 text-fg-3" strokeWidth={1.75} />}
          </span>
        );
      })}
    </div>
  );
}

/** The 48px top bar of the inset: trigger, separator, breadcrumb, then what goes on the right. */
export function TopBar({ crumbs, children, className }: { crumbs: string[]; children?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex h-12 shrink-0 items-center gap-2 border-b border-line px-4", className)}>
      <SidebarTrigger />
      <span className="mr-2 h-4 w-px shrink-0 bg-line-2" />
      <Breadcrumb items={crumbs} />
      {children ? <div className="ml-auto flex shrink-0 items-center gap-2">{children}</div> : null}
    </div>
  );
}

/** The search field of the top bar, with its shortcut. */
export function SearchField({ placeholder = "Search…", className, mono = false }: { placeholder?: string; className?: string; mono?: boolean }) {
  return (
    <span className={cn("flex h-8 min-w-0 items-center gap-2 rounded-sm border border-line-2 bg-ink-1 pr-1.5 pl-2.5 text-sm text-fg-3", className)}>
      <SearchIcon className="size-4 shrink-0" strokeWidth={1.75} />
      <span className={cn("min-w-0 flex-1 truncate", mono && "font-mono text-xs text-fg-2")}>{placeholder}</span>
      <Kbd>⌘K</Kbd>
    </span>
  );
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd className={cn("inline-flex h-5 shrink-0 items-center rounded-sm border border-line bg-ink-2 px-1 font-mono text-xs text-fg-3", className)}>
      {children}
    </kbd>
  );
}

const buttonTones = {
  primary: "bg-accent font-medium text-accent-ink",
  outline: "border border-line-2 bg-ink-1 text-fg-1",
  secondary: "bg-ink-3 text-fg",
  ghost: "text-fg-2",
} as const;

/** A button: 4px corners, 32px (28px small). Green only as `primary`. */
export function Button({
  children,
  tone = "outline",
  size = "md",
  icon: Icon,
  className,
}: {
  children?: ReactNode;
  tone?: keyof typeof buttonTones;
  size?: "sm" | "md";
  icon?: LucideIcon;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-sm whitespace-nowrap transition-colors duration-200",
        size === "md" ? "h-8 px-3 text-sm" : "h-7 px-2.5 text-[0.8125rem]",
        !children && (size === "md" ? "w-8 px-0" : "w-7 px-0"),
        buttonTones[tone],
        className,
      )}
    >
      {Icon ? <Icon className="size-4 shrink-0" strokeWidth={1.75} /> : null}
      {children}
    </span>
  );
}

const badgeTones = {
  outline: "border-line-2 text-fg-1",
  muted: "border-line text-fg-3",
  positive: "border-accent/25 bg-accent/10 text-accent",
  danger: "border-danger/30 bg-danger/10 text-danger",
  amber: "border-amber/30 bg-amber/10 text-amber",
} as const;

/** A badge: 4px corners, 20px tall, 12px text. */
export function Badge({ children, tone = "outline", className }: { children: ReactNode; tone?: keyof typeof badgeTones; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-5 shrink-0 items-center gap-1 rounded-sm border px-1.5 text-xs font-medium whitespace-nowrap transition-colors duration-300",
        badgeTones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

/** A checkbox: 16px, 4px corners; ticked in fg (selection is ink, not green). */
export function Checkbox({ checked }: { checked: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex size-4 shrink-0 items-center justify-center rounded-sm border transition-colors duration-200",
        checked ? "border-fg bg-fg text-ink-0" : "border-line-2 bg-ink-0",
      )}
    >
      <CheckIcon className={cn("size-3 transition-opacity", checked ? "opacity-100" : "opacity-0")} strokeWidth={3} />
    </span>
  );
}

/** A card: 6px corners, ink-1, a 1px line. */
export function Card({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("min-w-0 rounded-md border border-line bg-ink-1", className)}>{children}</div>;
}

/** A metric card: label, value and a line under it. */
export function Stat({ label, value, note, aside, className }: { label: string; value: ReactNode; note?: ReactNode; aside?: ReactNode; className?: string }) {
  return (
    <div className={cn("min-w-0", className)}>
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-[0.8125rem] text-fg-2">{label}</span>
        {aside}
      </div>
      <div className="mt-1.5 text-2xl leading-tight font-semibold tracking-[-0.02em] text-fg tabular-nums">{value}</div>
      {note ? <div className="mt-1 truncate text-[0.8125rem] text-fg-3">{note}</div> : null}
    </div>
  );
}

/** Segmented tabs: a 6px track with 4px tabs, the current one raised in ink. */
export function Segmented({ items, active, className, mono = false }: { items: string[]; active: number; className?: string; mono?: boolean }) {
  return (
    <span className={cn("inline-flex h-8 shrink-0 items-center gap-0.5 rounded-md border border-line bg-ink-1 p-0.5", className)}>
      {items.map((item, i) => (
        <span
          key={item}
          className={cn(
            "inline-flex h-full items-center rounded-sm px-2.5 text-[0.8125rem]",
            mono && "font-mono text-xs",
            i === active ? "bg-ink-3 font-medium text-fg" : "text-fg-3",
          )}
        >
          {item}
        </span>
      ))}
    </span>
  );
}

/** A status dot (the one round shape app UI keeps). */
export function StatusDot({ tone = "positive", pulse = false, className }: { tone?: "positive" | "neutral" | "danger" | "amber"; pulse?: boolean; className?: string }) {
  const color = { positive: "bg-accent", neutral: "bg-fg-3", danger: "bg-danger", amber: "bg-amber" }[tone];
  return (
    <span className={cn("relative inline-flex size-2 shrink-0", className)}>
      {pulse ? <span className={cn("absolute inset-0 rounded-full opacity-50 motion-safe:animate-ping", color)} /> : null}
      <span className={cn("relative size-2 rounded-full", color)} />
    </span>
  );
}
