import nextManifest from "../../../public/images/crm/next/manifest.json";
import nuxtManifest from "../../../public/images/crm/nuxt/manifest.json";

/**
 * The CRM showcase's data: the two front ends, their screenshots (captured
 * at 2x from the running apps against crm-api and its demo data; see each
 * `public/images/crm/<ui>/manifest.json`) and what each screen shows.
 */

export type ShotVariant = { src: string; width: number };

export type Shot = {
  key: string;
  src: string;
  width: number;
  height: number;
  alt: string;
  viewport: "desktop" | "phone";
  srcset: ShotVariant[];
};

export type UiId = "next" | "nuxt";

export type FrontEnd = {
  id: UiId;
  /** The framework's name, as on the toggle. */
  name: string;
  /** Where it lives in the repository. */
  app: string;
  /** Where `pnpm dev` serves it. */
  origin: string;
  /** What it is built with, in a line. */
  stack: string;
  /** The catch-all route that forwards `/api/*` to Navree. */
  proxyFile: string;
  shots: Record<string, Shot>;
};

function byKey(images: Shot[]): Record<string, Shot> {
  return Object.fromEntries(images.map((image) => [image.key, image]));
}

export const frontEnds: Record<UiId, FrontEnd> = {
  next: {
    id: "next",
    name: "Next.js",
    app: "apps/crm-web",
    origin: "localhost:3000",
    stack: "Next.js 16, React, TanStack Query, shadcn/ui",
    proxyFile: "src/app/api/[...path]/route.ts",
    shots: byKey(nextManifest.images as Shot[]),
  },
  nuxt: {
    id: "nuxt",
    name: "Nuxt",
    app: "apps/crm-nuxt",
    origin: "localhost:3102",
    stack: "Nuxt 4, Vue 3, TanStack Query, shadcn-vue",
    proxyFile: "server/api/[...path].ts",
    shots: byKey(nuxtManifest.images as Shot[]),
  },
};

export const uiIds: UiId[] = ["next", "nuxt"];

export type Screen = {
  key: string;
  label: string;
  /** The path in the address bar. */
  path: string;
  title: string;
  text: string;
  /** The API call behind the screen. */
  api: string;
};

/** The desktop screens of the gallery, in tab order. */
export const screens: Screen[] = [
  {
    key: "dashboard",
    label: "Dashboard",
    path: "/",
    title: "The dashboard",
    text: "Open pipeline, a forecast weighted by stage probability, deals won this month, the pipeline by stage and today's tasks, in one call.",
    api: "GET /api/v1/dashboard",
  },
  {
    key: "pipeline",
    label: "Pipeline",
    path: "/deals",
    title: "The deal pipeline",
    text: "Drag a card to change its stage, or move it with the keyboard. The move commits in one transaction and reaches every other open board over the WebSocket.",
    api: "GET /api/v1/deals/board",
  },
  {
    key: "deal",
    label: "Deal",
    path: "/deals/01a17b39-1983-7444-8e18-a1cb907c00d2",
    title: "A deal",
    text: "Stage, value and weighted value, the main contact, and a timeline of notes, calls, emails, meetings and tasks. Buttons a role can't use aren't shown.",
    api: "GET /api/v1/deals/{id}",
  },
  {
    key: "contacts",
    label: "Contacts",
    path: "/contacts",
    title: "Contacts",
    text: "Search, filter by status and owner, and export to CSV. The list only ever contains rows the signed-in user may read.",
    api: "GET /api/v1/contacts",
  },
  {
    key: "companies",
    label: "Companies",
    path: "/companies",
    title: "Companies",
    text: "Industries, head counts, locations and owners, sortable and filterable, with soft deletes behind the scenes.",
    api: "GET /api/v1/companies",
  },
  {
    key: "palette",
    label: "Command palette",
    path: "/",
    title: "The command palette",
    text: "Ctrl K jumps to a screen, creates a record or searches companies, contacts and deals at once, five of each, through the API.",
    api: "GET /api/v1/{companies,contacts,deals}?search=",
  },
];

/** The two phone views. */
export const phoneScreens: { key: string; label: string }[] = [
  { key: "phone-dashboard", label: "Dashboard" },
  { key: "phone-deal", label: "Deal" },
];

/** `srcset` for a shot: every size that was exported. */
export function srcSet(shot: Shot): string {
  return shot.srcset.map((v) => `${v.src} ${v.width}w`).join(", ");
}
