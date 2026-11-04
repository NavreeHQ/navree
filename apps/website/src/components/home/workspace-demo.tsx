"use client";

import {
  ArrowDownIcon,
  AudioWaveformIcon,
  Building2Icon,
  CheckIcon,
  ChevronRightIcon,
  ChevronsUpDownIcon,
  CommandIcon,
  ContactIcon,
  GalleryVerticalEndIcon,
  HandshakeIcon,
  LockIcon,
  PanelLeftIcon,
  PlusIcon,
  SettingsIcon,
  UserPlusIcon,
  UsersIcon,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { Avatar, Badge, LogoTile, SidebarItem, type NavGroup } from "@/components/ecosystem/app-shell";
import { cn } from "@/lib/utils";

type Deal = { title: string; company: string; amount: string; stage: string };
type Space = { slug: string; name: string; logo: LucideIcon; plan: string; role: string; members: string[]; deals: Deal[] };

/** Three workspaces of one app. Each sees only its own deals (four each, so switching never changes the height). */
const spaces: Space[] = [
  {
    slug: "sutlej",
    name: "Sutlej Supply",
    logo: GalleryVerticalEndIcon,
    plan: "Pro plan",
    role: "Owner",
    members: ["SK", "GS", "HK", "RB", "PV"],
    deals: [
      { title: "Fleet renewal", company: "Majha Motors", amount: "₹24,00,000", stage: "Won" },
      { title: "Second warehouse", company: "Ravi Freight Lines", amount: "₹13,50,000", stage: "Proposal" },
      { title: "Pilot, three clinics", company: "Shivalik Dental Care", amount: "₹5,90,000", stage: "Qualified" },
      { title: "Support renewal", company: "Chenab Analytics", amount: "₹8,50,000", stage: "Lead" },
    ],
  },
  {
    slug: "haveli",
    name: "Haveli Goods",
    logo: AudioWaveformIcon,
    plan: "Business plan",
    role: "Admin",
    members: ["JD", "SK", "KB"],
    deals: [
      { title: "Rail sensors, phase 2", company: "Doaba Rail Components", amount: "₹72,00,000", stage: "Negotiation" },
      { title: "Cold-chain audit", company: "Pind Pantry", amount: "₹9,80,000", stage: "Won" },
      { title: "Depot telemetry", company: "Grand Trunk Transit", amount: "₹16,40,000", stage: "Won" },
      { title: "Fleet tracking pilot", company: "Kesar Logistics", amount: "₹5,75,000", stage: "Proposal" },
    ],
  },
  {
    slug: "doaba",
    name: "Doaba Traders",
    logo: CommandIcon,
    plan: "Free plan",
    role: "Viewer",
    members: ["MG", "NG", "RM", "AS", "AA", "SK", "IG", "MJ"],
    deals: [
      { title: "Print fleet lease", company: "Rangla Print House", amount: "₹4,30,000", stage: "Lead" },
      { title: "Guest Wi-Fi rollout", company: "Deodar Hospitality", amount: "₹22,50,000", stage: "Proposal" },
      { title: "Data migration", company: "Phulkari Textiles", amount: "₹7,60,000", stage: "Qualified" },
      { title: "Kiosk refresh", company: "Roshni Cinemas", amount: "₹3,80,000", stage: "Won" },
    ],
  },
];

const DOMAIN = "app.example.com";
const USER = { initials: "SK", name: "Simran Kaur", email: "simran@example.com" };

const rupees = new Intl.NumberFormat("en-IN", { style: "currency", currency: "INR", maximumFractionDigits: 0 });
const sum = (deals: Deal[]) => rupees.format(deals.reduce((total, d) => total + Number(d.amount.replace(/[₹,]/g, "")), 0));

function nav(space: Space): NavGroup[] {
  return [
    {
      label: "Sales",
      items: [
        { icon: HandshakeIcon, label: "Deals", badge: String(space.deals.length), active: true },
        { icon: Building2Icon, label: "Companies" },
        { icon: ContactIcon, label: "Contacts" },
      ],
    },
    {
      label: "Workspace",
      items: [
        { icon: UsersIcon, label: "Members", badge: String(space.members.length) },
        { icon: SettingsIcon, label: "Settings" },
      ],
    },
  ];
}

/** The switcher's button: the workspace's tile, name and plan (just the tile when collapsed). */
function SwitcherButton({
  space,
  open,
  collapsed,
  compact,
  onClick,
  menuId,
}: {
  space: Space;
  open: boolean;
  collapsed?: boolean;
  /** The small version in the top bar, on narrow frames. */
  compact?: boolean;
  onClick: (button: HTMLButtonElement) => void;
  menuId: string;
}) {
  const Logo = space.logo;
  return (
    <button
      type="button"
      onClick={(event) => onClick(event.currentTarget)}
      aria-expanded={open}
      aria-controls={menuId}
      aria-label={`Workspace: ${space.name}. Switch workspace`}
      className={cn(
        "flex cursor-pointer items-center rounded-sm text-left transition-colors duration-200 hover:bg-ink-2 aria-expanded:bg-ink-2",
        collapsed ? "size-8 justify-center" : compact ? "h-8 min-w-0 gap-2 px-1.5" : "h-12 w-full gap-2 p-2",
      )}
    >
      <LogoTile size={compact ? "sm" : "md"}>
        <Logo strokeWidth={2} />
      </LogoTile>
      {collapsed ? null : compact ? (
        <span className="min-w-0 truncate text-sm font-medium text-fg">{space.name}</span>
      ) : (
        <span className="grid min-w-0 flex-1 leading-tight">
          <span className="truncate text-sm font-medium text-fg">{space.name}</span>
          <span className="truncate text-xs text-fg-3">{space.plan}</span>
        </span>
      )}
      {collapsed ? null : <ChevronsUpDownIcon aria-hidden className="size-4 shrink-0 text-fg-3" strokeWidth={1.75} />}
    </button>
  );
}

/** The sidebar: switcher, the workspace's navigation and the user. */
function Sidebar({ space, collapsed, switcher }: { space: Space; collapsed: boolean; switcher: ReactNode }) {
  return (
    <div className={cn("flex h-full min-h-0 flex-col", collapsed && "items-center")}>
      <div className="p-2">{switcher}</div>
      <div aria-hidden className="flex min-h-0 flex-1 flex-col">
        {nav(space).map((group) => (
          <div key={group.label} className="flex flex-col p-2">
            {collapsed ? null : <div className="flex h-8 items-center px-2 text-xs text-fg-3">{group.label}</div>}
            <div className="flex flex-col gap-1">
              {group.items.map((item) => (
                <SidebarItem key={item.label} item={item} collapsed={collapsed} />
              ))}
            </div>
          </div>
        ))}
      </div>
      <div aria-hidden className="p-2">
        {collapsed ? (
          <Avatar initials={USER.initials} />
        ) : (
          <div className="flex h-12 items-center gap-2 p-2">
            <Avatar initials={USER.initials} />
            <div className="grid min-w-0 flex-1 leading-tight">
              <span className="truncate text-sm font-medium text-fg">{USER.name}</span>
              <span className="truncate text-xs text-fg-3">{USER.email}</span>
            </div>
            <ChevronsUpDownIcon className="size-4 shrink-0 text-fg-3" strokeWidth={1.75} />
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * The workspace spotlight's live part: an app with a working workspace
 * switcher and sidebar (the trigger or Ctrl/Cmd+B collapses it; on narrow
 * frames it opens as a sheet), and the resolution panel that shows how the
 * request names the workspace. Switching changes the URL, the role and the
 * rows, never the layout (every view reserves the same space).
 */
export function WorkspaceDemo() {
  const [index, setIndex] = useState(0);
  const [menu, setMenu] = useState<"closed" | "sidebar" | "topbar">("closed");
  const [collapsed, setCollapsed] = useState(false);
  const [sheet, setSheet] = useState(false);
  const app = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement | null>(null);
  const space = spaces[index];
  const menuId = "workspace-menu";

  // Close the menu on a click outside it.
  useEffect(() => {
    if (menu === "closed") return;
    const onDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (menuRef.current?.contains(target) || opener.current?.contains(target)) return;
      setMenu("closed");
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [menu]);

  function toggleMenu(from: "sidebar" | "topbar", el: HTMLButtonElement) {
    opener.current = el;
    setMenu((m) => (m === from ? "closed" : from));
  }

  function pick(i: number) {
    setIndex(i);
    setMenu("closed");
    setSheet(false);
    opener.current?.focus();
  }

  /** The trigger collapses the sidebar on wide frames and opens the sheet on narrow ones. */
  function toggleSidebar() {
    setMenu("closed");
    if ((app.current?.offsetWidth ?? 0) >= 672) setCollapsed((c) => !c);
    else setSheet((s) => !s);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" && (menu !== "closed" || sheet)) {
      event.preventDefault();
      setMenu("closed");
      setSheet(false);
      opener.current?.focus();
    } else if (event.key.toLowerCase() === "b" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      toggleSidebar();
    }
  }

  const resolvers = [
    { kind: "Subdomain", value: `${space.slug}.${DOMAIN}`, matched: true },
    { kind: "Header", value: `X-Workspace: ${space.slug}` },
    { kind: "Path", value: `/api/w/${space.slug}/deals` },
    { kind: "Token claim", value: `"workspace": "${space.slug}"` },
  ];
  const canEdit = space.role !== "Viewer";
  const sidebarButton = (isCollapsed: boolean) => (
    <SwitcherButton
      space={space}
      open={menu === "sidebar"}
      collapsed={isCollapsed}
      onClick={(el) => toggleMenu("sidebar", el)}
      menuId={menuId}
    />
  );

  return (
    <div className="grid gap-3 lg:grid-cols-12">
      {/* The app. */}
      <div className="bezel flex min-w-0 flex-col lg:col-span-7">
        <div className="bezel-core flex flex-1 flex-col">
          <div className="grid h-10 shrink-0 grid-cols-[minmax(max-content,1fr)_minmax(0,24rem)_minmax(max-content,1fr)] items-center gap-3 border-b border-line bg-ink-1 px-3.5">
            <div className="flex gap-1.5" aria-hidden>
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
              <span className="size-2.5 rounded-full bg-line-2" />
            </div>
            <div className="flex h-6 min-w-0 items-center justify-center gap-1.5 rounded-sm border border-line bg-ink-0/70 px-2.5 font-mono text-xs text-fg-3">
              <LockIcon aria-hidden className="size-3 shrink-0" strokeWidth={1.75} />
              <span className="truncate">
                <span className="text-fg">{space.slug}</span>.{DOMAIN}/deals
              </span>
            </div>
            <span aria-hidden />
          </div>

          <div ref={app} onKeyDown={onKeyDown} className="@container relative flex flex-1">
            <div className={cn("grid flex-1 bg-ink-0", collapsed ? "@2xl:grid-cols-[3rem_minmax(0,1fr)]" : "@2xl:grid-cols-[16rem_minmax(0,1fr)]")}>
              {/* The sidebar, on wide frames: expanded or the icon rail. */}
              <nav aria-label="App" className="hidden min-h-0 flex-col border-r border-line bg-ink-1 @2xl:flex">
                <Sidebar space={space} collapsed={collapsed} switcher={sidebarButton(collapsed)} />
              </nav>

              {/* The inset. */}
              <div className="flex min-w-0 flex-col">
                <div className="flex h-12 shrink-0 items-center gap-2 border-b border-line px-3 @md:px-4">
                  <button
                    type="button"
                    onClick={toggleSidebar}
                    aria-label="Toggle the sidebar"
                    aria-keyshortcuts="Control+B Meta+B"
                    className="-ml-1 inline-flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-sm text-fg-2 transition-colors duration-200 hover:bg-ink-2 hover:text-fg"
                  >
                    <PanelLeftIcon aria-hidden className="size-4" strokeWidth={1.75} />
                  </button>
                  <span aria-hidden className="mr-1 h-4 w-px shrink-0 bg-line-2 @md:mr-2" />
                  {/* Narrow frames: the switcher lives in the top bar. */}
                  <div className="flex min-w-0 @2xl:hidden">
                    <SwitcherButton
                      space={space}
                      compact
                      open={menu === "topbar"}
                      onClick={(el) => toggleMenu("topbar", el)}
                      menuId={menuId}
                    />
                  </div>
                  <div aria-hidden className="hidden min-w-0 items-center gap-1.5 text-sm @2xl:flex">
                    <span className="truncate text-fg-3">{space.name}</span>
                    <ChevronRightIcon className="size-3.5 shrink-0 text-fg-3" strokeWidth={1.75} />
                    <span className="text-fg">Deals</span>
                  </div>
                  <div className="ml-auto flex shrink-0 items-center gap-2">
                    <Badge tone="outline" className="font-normal text-fg-2">
                      {space.role}
                    </Badge>
                  </div>
                </div>

                {/* The deals of the current workspace only. */}
                <div className="flex min-w-0 flex-1 flex-col p-4 @md:p-5">
                  <p className="sr-only" aria-live="polite">
                    {space.name}: {space.deals.length} deals, you are {space.role}.
                  </p>
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-xl font-semibold tracking-[-0.01em] text-fg">Deals</p>
                      <p className="mt-1 truncate text-[0.8125rem] text-fg-3">
                        {space.deals.filter((d) => d.stage !== "Won").length} open · you are {space.role} in {space.name}
                      </p>
                    </div>
                    {/* The role decides what the screen offers: viewers can't create. */}
                    <span
                      aria-hidden
                      className={cn(
                        "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-sm px-3 text-sm",
                        canEdit ? "bg-accent font-medium text-accent-ink" : "border border-line text-fg-3",
                      )}
                    >
                      {canEdit ? <PlusIcon className="size-4" strokeWidth={2} /> : <LockIcon className="size-3.5" strokeWidth={1.75} />}
                      {canEdit ? "New deal" : "Read only"}
                    </span>
                  </div>

                  {/* The workspace's own totals: they change with the switcher, like everything else. */}
                  <dl className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-md border border-line bg-line">
                    {[
                      { label: "Open pipeline", value: sum(space.deals.filter((d) => d.stage !== "Won")) },
                      { label: "Won this quarter", value: sum(space.deals.filter((d) => d.stage === "Won")) },
                    ].map((stat) => (
                      <div key={stat.label} className="bg-ink-1 px-3.5 py-3">
                        <dt className="truncate text-[0.8125rem] text-fg-2">{stat.label}</dt>
                        <dd className="mt-1 text-xl leading-tight font-semibold tracking-[-0.02em] text-fg tabular-nums">{stat.value}</dd>
                      </div>
                    ))}
                  </dl>

                  <ul key={space.slug} className="mt-3 overflow-hidden rounded-md border border-line">
                    {space.deals.map((deal, i) => (
                      <li
                        key={deal.title}
                        className="row-in grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 border-b border-line px-3 py-2.5 last:border-b-0 @md:grid-cols-[minmax(0,1fr)_6.5rem_5.5rem]"
                        style={{ animationDelay: `${i * 45}ms` }}
                      >
                        <span className="min-w-0">
                          <span className="block truncate text-sm text-fg">{deal.title}</span>
                          <span className="block truncate text-[0.8125rem] text-fg-3">{deal.company}</span>
                        </span>
                        <span className="hidden @md:block">
                          <Badge tone={deal.stage === "Won" ? "positive" : "outline"}>
                            {deal.stage === "Won" ? <CheckIcon aria-hidden className="size-3" strokeWidth={2.5} /> : null}
                            {deal.stage}
                          </Badge>
                        </span>
                        <span className="text-right">
                          <span className={cn("block text-sm tabular-nums", deal.stage === "Won" ? "text-accent" : "text-fg-1")}>{deal.amount}</span>
                          <span className="block text-[0.8125rem] text-fg-3 @md:hidden">{deal.stage}</span>
                        </span>
                      </li>
                    ))}
                  </ul>

                  {/* The members of this workspace, and an invitation waiting to go out. */}
                  <div className="mt-auto pt-4">
                    <div className="flex items-center justify-between gap-3 rounded-md border border-line bg-ink-1 py-2 pr-2 pl-3">
                      <div className="flex min-w-0 items-center gap-3">
                        <div className="flex gap-1" aria-hidden>
                          {space.members.slice(0, 4).map((m) => (
                            <Avatar key={m} initials={m} size="sm" />
                          ))}
                        </div>
                        <span className="truncate text-[0.8125rem] text-fg-3">{space.members.length} members</span>
                      </div>
                      {canEdit ? (
                        <span className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-sm border border-line-2 bg-ink-1 px-2.5 text-[0.8125rem] text-fg-1">
                          <UserPlusIcon aria-hidden className="size-3.5" strokeWidth={1.75} />
                          <span>
                            Invite<span className="hidden @md:inline"> by email</span>
                          </span>
                        </span>
                      ) : (
                        <span className="shrink-0 px-2 text-[0.8125rem] text-fg-3">Only admins invite</span>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            </div>

            {/* Narrow frames: the sidebar as an off-canvas sheet. */}
            <div
              aria-hidden
              onClick={() => setSheet(false)}
              className={cn(
                "absolute inset-0 z-10 bg-ink-0/60 transition-opacity duration-300 @2xl:hidden",
                sheet ? "opacity-100" : "pointer-events-none opacity-0",
              )}
            />
            <nav
              aria-label="App"
              className={cn(
                "absolute inset-y-0 left-0 z-10 w-64 border-r border-line bg-ink-1 transition-[translate,visibility] duration-300 ease-(--ease-out-quint) @2xl:hidden",
                sheet ? "visible translate-x-0" : "invisible -translate-x-full",
              )}
            >
              <Sidebar
                space={space}
                collapsed={false}
                switcher={
                  <SwitcherButton
                    space={space}
                    open={menu === "sidebar"}
                    onClick={(el) => toggleMenu("sidebar", el)}
                    menuId={menuId}
                  />
                }
              />
            </nav>

            {/* The switcher's menu. */}
            {menu !== "closed" ? (
              <div
                ref={menuRef}
                id={menuId}
                className={cn(
                  "absolute z-20 w-60 rounded-md border border-line-2 bg-ink-1 p-1 shadow-float motion-safe:animate-in motion-safe:fade-in-0 motion-safe:zoom-in-95",
                  menu === "topbar" ? "top-11 left-12" : collapsed ? "top-11 left-2 @max-2xl:top-[3.75rem]" : "top-[3.75rem] left-2",
                )}
              >
                <p className="flex h-8 items-center px-2 text-xs text-fg-3">Workspaces</p>
                <ul aria-label="Switch workspace">
                  {spaces.map((s, i) => {
                    const Logo = s.logo;
                    return (
                      <li key={s.slug}>
                        <button
                          type="button"
                          aria-pressed={i === index}
                          onClick={() => pick(i)}
                          className="flex h-8 w-full cursor-pointer items-center gap-2 rounded-sm px-2 text-left text-sm text-fg-1 transition-colors duration-150 hover:bg-ink-3 hover:text-fg focus-visible:bg-ink-3"
                        >
                          <LogoTile size="sm" tone="outline">
                            <Logo strokeWidth={2} />
                          </LogoTile>
                          <span className="min-w-0 flex-1 truncate">{s.name}</span>
                          <span className="text-xs text-fg-3">{s.role}</span>
                          <CheckIcon aria-hidden className={cn("size-4 shrink-0 text-fg-2", i !== index && "invisible")} strokeWidth={2} />
                        </button>
                      </li>
                    );
                  })}
                </ul>
                <div aria-hidden className="-mx-1 my-1 h-px bg-line" />
                <div aria-hidden className="flex h-8 items-center gap-2 px-2 text-sm text-fg-3">
                  <span className="inline-flex size-6 items-center justify-center rounded-sm border border-dashed border-line-2">
                    <PlusIcon className="size-3.5" strokeWidth={1.75} />
                  </span>
                  Create workspace
                </div>
              </div>
            ) : null}
          </div>
        </div>
      </div>

      {/* How the request named the workspace. */}
      <div className="surface flex min-w-0 flex-col p-5 sm:p-6 lg:col-span-5">
        <h3 className="font-display text-display-sm font-semibold text-fg">Resolved from the request</h3>
        <p className="mt-2 text-[0.9375rem] leading-relaxed text-fg-2">
          From the subdomain, a header, a path prefix or a token claim, in the order you configure. The first match wins.
        </p>
        <ul className="mt-5 divide-y divide-line rounded-md border border-line bg-ink-0/60">
          {resolvers.map((r) => (
            <li key={r.kind} className="grid grid-cols-[5.75rem_minmax(0,1fr)] items-center gap-3 px-3.5 py-2.5 sm:grid-cols-[6.5rem_minmax(0,1fr)]">
              <span className="text-[0.8125rem] text-fg-2">{r.kind}</span>
              <span className="flex min-w-0 items-center justify-between gap-2">
                <code className="truncate font-mono text-[0.78rem] text-fg-1">{r.value}</code>
                {r.matched ? (
                  <Badge tone="positive" className="font-mono font-normal">
                    <CheckIcon aria-hidden className="size-3" strokeWidth={2.5} />
                    <span className="max-sm:sr-only">match</span>
                  </Badge>
                ) : null}
              </span>
            </li>
          ))}
        </ul>
        <div aria-hidden className="flex justify-center py-2.5 text-fg-3">
          <ArrowDownIcon className="size-4" />
        </div>
        <div className="rounded-md border border-line-2 bg-ink-2 px-4 py-3.5 font-mono text-[0.78rem] leading-relaxed">
          <p className="text-fg">CurrentWorkspace</p>
          <dl className="mt-1.5 grid grid-cols-[6rem_minmax(0,1fr)] gap-y-0.5 text-fg-3">
            <dt>workspace</dt>
            <dd className="truncate text-fg-1">{space.name}</dd>
            <dt>role</dt>
            <dd className="text-fg-1">{space.role}</dd>
            <dt>isolation</dt>
            <dd className="text-fg-1">row</dd>
          </dl>
        </div>
        <p className="mt-4 text-sm leading-relaxed text-fg-3">
          Unknown workspace: <span className="font-mono text-fg-2">404</span>. Not a member: <span className="font-mono text-fg-2">403</span>.
          Both in the usual JSON error shape.
        </p>
      </div>
    </div>
  );
}
