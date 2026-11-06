import { CheckIcon, ChevronRightIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { DiagramFigure, diagrams } from "@/components/diagrams";
import { ArrowLink, PrimaryCta, SecondaryCta, SectionHeading } from "@/components/home/parts";
import { CodePanel, CommandLine } from "@/components/showcase/code-panel";
import { frontEnds } from "@/components/showcase/crm-data";
import { crmCrates, crmFrontEnds } from "@/components/showcase/crm-diagrams";
import { CrmGallery } from "@/components/showcase/crm-gallery";
import { frameworkModuleCount } from "@/components/showcase/crm-source";
import { BrowserFrame, PhoneFrame, ShotImage } from "@/components/showcase/frames";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";
import { cn } from "@/lib/utils";
import { site } from "@site";

export const metadata: Metadata = pageMetadata({
  title: "A real CRM, built on Navree",
  description:
    "The reference CRM on a Navree API: companies, contacts, a deal pipeline, tasks and a dashboard, with Next.js and Nuxt front ends. Screens and run commands.",
  path: "/showcase/crm/",
});

/** A section of this page: a hairline on top, the container and the rhythm. */
function Section({ id, children, className }: { id: string; children: ReactNode; className?: string }) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-heading`}
      className={cn("cv-auto scroll-mt-(--header-h) border-t border-line", className)}
    >
      <div className="container-site section-y">{children}</div>
    </section>
  );
}

/** A sub-section heading inside "How it is built". */
function ChapterHeading({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  return (
    <div>
      <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">{label}</p>
      <h3 className="mt-3 font-display text-display-md font-semibold text-balance text-fg">{title}</h3>
      <div className="mt-4 max-w-2xl space-y-4 text-[0.9375rem] leading-relaxed text-pretty text-fg-2 sm:text-base">
        {children}
      </div>
    </div>
  );
}

/** A list with green checks, for what a feature does. */
function Checks({ items, className }: { items: ReactNode[]; className?: string }) {
  return (
    <ul className={cn("mt-6", className)}>
      {items.map((item, i) => (
        <li key={i} className="flex gap-3 border-t border-line py-2.5 text-[0.9375rem] leading-snug text-pretty text-fg-1">
          <CheckIcon aria-hidden className="mt-[0.2rem] size-4 shrink-0 text-accent/80" strokeWidth={2} />
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

const Code = ({ children }: { children: ReactNode }) => <code className="inline-code">{children}</code>;

/* ------------------------------------------------------------------ hero */

const facts = [
  { value: "8", label: "CRM modules", text: "Four feature crates on one shared core" },
  { value: String(frameworkModuleCount()), label: "framework modules", text: "HTTP to real time, configured once" },
  { value: "3", label: "roles", text: "Checked on routes, in SQL and in the browser" },
  { value: "2", label: "front ends", text: "Next.js and Nuxt on one BFF contract" },
];

function Hero() {
  const next = frontEnds.next;
  return (
    <section aria-labelledby="crm-heading" className="relative isolate overflow-hidden">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-0 top-[22rem] -z-10 mx-auto h-[44rem] max-w-[80rem] bg-[radial-gradient(ellipse_at_center,rgb(61_255_143/0.11),transparent_62%)] sm:top-[20rem] lg:top-[18rem]"
      />
      <div className="container-site pt-10 sm:pt-14 lg:pt-16">
        <nav aria-label="Breadcrumb" className="mb-10 lg:mb-14">
          <ol className="flex items-center gap-1.5 font-mono text-xs text-fg-3">
            <li>
              <Link href="/showcase/" className="rounded-sm transition-colors hover:text-fg">
                Showcase
              </Link>
            </li>
            <li aria-hidden>
              <ChevronRightIcon className="size-3.5" />
            </li>
            <li aria-current="page" className="text-fg-2">
              CRM
            </li>
          </ol>
        </nav>

        <div className="grid gap-x-12 gap-y-8 lg:grid-cols-12 lg:items-end">
          <div className="lg:col-span-7">
            <p className="eyebrow">Reference app</p>
            <h1 id="crm-heading" className="mt-5 text-display-xl font-semibold text-balance text-fg">
              <span className="block">A real CRM,</span>
              <span className="block text-fg-2">built on Navree.</span>
            </h1>
          </div>
          <div className="lg:col-span-5 lg:pb-2">
            <p className="max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
              Companies, contacts, a deal pipeline, tasks and a live dashboard for a sales team with roles and an audit
              trail. The API is a Navree app split into feature crates, and it ships with two front ends, Next.js and
              Nuxt. It lives in the repository and is tested with the framework.
            </p>
            <div className="mt-8 flex flex-wrap items-center gap-3">
              <PrimaryCta href="#run">Run it locally</PrimaryCta>
              <SecondaryCta href="/docs/example-app/crm-tour/">Read the tour</SecondaryCta>
            </div>
          </div>
        </div>

        <div className="relative mt-12 sm:mt-16 lg:mt-20 lg:pr-24 xl:pr-32">
          <BrowserFrame
            address={`${next.origin}/`}
            bar={<span className="hidden font-mono text-xs text-fg-3 sm:inline">{next.app}</span>}
            className="shadow-glow-lg"
          >
            <ShotImage
              shot={next.shots.dashboard}
              priority
              sizes="(min-width: 1440px) 1250px, (min-width: 1024px) calc(100vw - 180px), (min-width: 640px) calc(100vw - 64px), calc(100vw - 48px)"
            />
          </BrowserFrame>
          <div className="absolute right-0 -bottom-10 hidden w-[15rem] lg:block xl:w-[16.5rem]">
            <PhoneFrame className="shadow-float">
              <ShotImage shot={next.shots["phone-deal"]} sizes="264px" />
            </PhoneFrame>
          </div>
        </div>
      </div>

      <div className="container-site mt-16 lg:mt-24">
        <dl className="grid grid-cols-2 border-t border-line lg:grid-cols-4">
          {facts.map((f, i) => (
            <div
              key={f.label}
              className={cn(
                "py-7 pr-4 sm:py-9 sm:pr-6",
                i % 2 === 1 && "border-l border-line pl-4 sm:pl-6",
                i >= 2 && "border-t border-line lg:border-t-0",
                i === 2 && "lg:border-l lg:pl-6",
              )}
            >
              <dt className="sr-only">{f.label}</dt>
              <dd>
                <span className="block font-display text-display-md font-semibold text-fg numeric">{f.value}</span>
                <span className="mt-1 block text-[0.9375rem] font-medium text-fg-1">{f.label}</span>
                <span className="mt-1 block text-[0.8125rem] leading-snug text-pretty text-fg-3">{f.text}</span>
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}

/* --------------------------------------------------------------- gallery */

function Screens() {
  return (
    <Section id="screens">
      <SectionHeading
        split
        id="screens-heading"
        eyebrow="The screens"
        title="Every screen, in both front ends."
        lead={
          <p>
            Captured from the running app against the CRM API and its demo data, signed in as the sales manager. Switch
            between the Next.js and the Nuxt front end: the same API, the same contract, the same theme.
          </p>
        }
      />
      <div className="mt-12 lg:mt-16">
        <CrmGallery />
      </div>
    </Section>
  );
}

/* ---------------------------------------------------------- how it works */

const APP_RS = `pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
        .module(CrmCoreModule)
        .module(UsersModule)
        .module(CompaniesModule)
        .module(ContactsModule)
        .module(DealsModule)
        .module(ActivitiesModule)
        .module(DashboardModule)
        .module(DemoModule)
}`;

const CONTROLLER_RS = `#[controller("/deals", version = "1")]
impl DealsController {
    #[get("/board")]
    #[can(Read, Deal)]
    async fn board(
        &self,
        uow: Uow,
        scope: Accessible<Deal, Read>,
        Valid(Query(filter)): Valid<Query<BoardFilter>>,
    ) -> Result<Json<DealBoard>> {
        Ok(Json(self.deals.board(&uow, scope, filter).await?))
    }

    #[post("/{id}/move")]
    #[can(Move, Deal, from = "id")]
    async fn move_deal(
        &self,
        uow: Uow,
        CurrentUser(user): CurrentUser,
        deal: Authorized<Deal, Move>,
        Valid(Json(input)): Valid<Json<MoveDeal>>,
    ) -> Result<Json<DealResource>> {
        let deal = self.deals.move_to(&uow, user.id, deal.into_inner(), input).await?;
        Ok(Json(deal))
    }
}`;

const EVENTS_RS = `impl DealsModule {
    /// The owner notifications (a queued listener of both events) and
    /// their mail template.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>()
            .templates(notification::TEMPLATES);
    }
}`;

const WORKSPACE_RS = `platform(Navree::app(), sources)
    .module(WorkspaceModule::from_config()) // [!code ++]
    .module(CrmCoreModule)
    .module(CompaniesModule)
    .module(DealsModule)`;

function Built() {
  return (
    <Section id="built">
      <SectionHeading
        split
        id="built-heading"
        eyebrow="How it is built"
        title="A Navree app, from crates to sockets."
        lead={
          <p>
            The CRM uses the framework the way your app will: feature modules on a shared core, access rules in one
            function, events and queues for side effects, real time over the Pusher protocol and a backend-for-frontend
            that keeps tokens out of the browser.
          </p>
        }
        action={<ArrowLink href="/docs/example-app/crm-tour/">Read the full tour</ArrowLink>}
      />

      {/* Modules */}
      <div className="mt-14 grid items-start gap-x-16 gap-y-10 lg:mt-20 xl:grid-cols-12">
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="01 · Modules" title="Feature crates on one core.">
            <p>
              Each feature is a crate with its own module, controller, service and tests. Feature crates depend only on{" "}
              <Code>crm-core</Code>, never on each other, so a deal can reach shared models but never the contacts code.
              Cargo builds and tests them in parallel.
            </p>
            <p>
              <Code>crm-core</Code> holds the ORM models and their factories, the migrations, the roles and the abilities function, and its{" "}
              <Code>platform()</Code> configures the framework modules once for the API and every test.
            </p>
          </ChapterHeading>
          <CodePanel file="apps/crm-api/src/app.rs" lang="rust" source={APP_RS} className="mt-8" />
        </div>
        <DiagramFigure spec={crmCrates} name="crm-crates" className="xl:col-span-7" />
      </div>

      {/* Access */}
      <div className="mt-20 grid items-start gap-x-16 gap-y-10 border-t border-line pt-16 lg:mt-28 lg:pt-20 xl:grid-cols-12">
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="02 · Access rules" title="Every rule in one function, enforced three ways.">
            <p>
              Three roles, <Code>admin</Code>, <Code>manager</Code> and <Code>rep</Code>, are stored in the access module&apos;s
              tables. One function, <Code>crm_core::abilities::define</Code>, says what each may do: read everything, create
              records, change what you own, never delete a won deal.
            </p>
          </ChapterHeading>
          <Checks
            items={[
              <>
                <strong className="font-medium text-fg">Routes</strong> declare what they need. <Code>#[can(Move, Deal, from = &quot;id&quot;)]</Code>{" "}
                loads the deal and checks it before the handler runs.
              </>,
              <>
                <strong className="font-medium text-fg">Lists</strong> take <Code>Accessible&lt;Deal, Read&gt;</Code>, which turns
                the rules into SQL, so a page only holds rows the user may read.
              </>,
              <>
                <strong className="font-medium text-fg">The browser</strong> gets the same rules as CASL JSON from{" "}
                <Code>GET /api/me/abilities</Code> and hides what a user can&apos;t do.
              </>,
            ]}
          />
        </div>
        <CodePanel
          file="crates/crm-deals/src/controller.rs"
          lang="rust"
          source={CONTROLLER_RS}
          className="xl:col-span-7"
        />
      </div>

      {/* Events, real time and workspaces */}
      <div className="mt-20 grid gap-3 border-t border-line pt-16 lg:mt-28 lg:grid-cols-2 lg:pt-20">
        <article className="flex min-w-0 flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5">
          <div className="px-4 pt-5 pb-6 sm:px-5">
            <ChapterHeading label="03 · Events and real time" title="A deal moves on every open board.">
              <p>
                After every change to a deal commits, the API broadcasts it as <Code>deal.updated</Code> on{" "}
                <Code>private-crm.deals</Code>, and every open board moves the card. Winning or reassigning a deal also
                emits <Code>DealWon</Code> or <Code>DealAssigned</Code>: a queued listener notifies the owner, stored for
                the bell, pushed to their browser and mailed.
              </p>
            </ChapterHeading>
          </div>
          <CodePanel file="crates/crm-deals/src/lib.rs" lang="rust" source={EVENTS_RS} className="mt-auto rounded-[0.875rem]" />
        </article>
        <article className="flex min-w-0 flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5">
          <div className="px-4 pt-5 pb-6 sm:px-5">
            <ChapterHeading label="04 · Workspaces" title="One team today, many tomorrow.">
              <p>
                The CRM runs one team&apos;s workspace. To serve many teams, add <Code>WorkspaceModule</Code> and give the
                tenant tables a <Code>workspace_id</Code>: requests resolve their workspace from a header, a subdomain, a
                path or a token claim, members get workspace roles, and every query stays inside its workspace. Routes,
                services and rules stay as they are.
              </p>
            </ChapterHeading>
            <div className="mt-6">
              <ArrowLink href="/docs/workspaces/introduction/">Read about workspaces</ArrowLink>
            </div>
          </div>
          <CodePanel file="apps/crm-api/src/app.rs" lang="rust" source={WORKSPACE_RS} className="mt-auto rounded-[0.875rem]" />
        </article>
      </div>

      {/* BFF */}
      <div className="mt-20 grid items-center gap-x-16 gap-y-10 border-t border-line pt-16 lg:mt-28 lg:pt-20 xl:grid-cols-12">
        <DiagramFigure
          spec={diagrams["backend-for-frontend"]}
          name="backend-for-frontend"
          className="max-xl:order-2 xl:col-span-7"
        />
        <div className="min-w-0 xl:col-span-5">
          <ChapterHeading label="05 · The BFF" title="No token ever reaches the browser.">
            <p>
              The browser calls only its own server. That server, the backend-for-frontend, keeps Navree&apos;s access token
              and rotating refresh token in httpOnly cookies, forwards each call with <Code>Authorization: Bearer</Code> and
              refreshes once per refresh token, because reusing a rotated one ends the session.
            </p>
            <p>
              The WebSocket goes straight to Navree and carries no token. Private channels are authorized through the BFF.
            </p>
          </ChapterHeading>
          <div className="mt-6">
            <ArrowLink href="/docs/patterns/backend-for-frontend/">The backend-for-frontend pattern</ArrowLink>
          </div>
        </div>
      </div>
    </Section>
  );
}

/* ------------------------------------------------------ one API, two UIs */

const NEXT_ROUTE = `import { bff } from "@/lib/bff/server";

/** Every \`/api/*\` call from the browser is forwarded to Navree with the bearer token from the httpOnly cookie. */
function handle(request: Request): Promise<Response> {
  return bff.proxy(request, new URL(request.url).pathname);
}

export const dynamic = "force-dynamic";
export { handle as DELETE, handle as GET, handle as HEAD, handle as PATCH, handle as POST, handle as PUT };`;

const NUXT_ROUTE = `import { useBff } from "../lib/instance";

/**
 * Every \`/api/*\` call from the browser goes to Navree with the bearer token
 * from the httpOnly cookie (refreshed once on a 401). Mutations must come from
 * this app's own origin.
 */
export default defineEventHandler((event) => {
  const request = toWebRequest(event);
  return useBff().proxy(request, new URL(request.url).pathname);
});`;

function FrontEnds() {
  const { next, nuxt } = frontEnds;
  return (
    <Section id="front-ends">
      <SectionHeading
        split
        id="front-ends-heading"
        eyebrow="One API, your front end"
        title="Next.js or Nuxt. Same contract, same theme."
        lead={
          <p>
            Both front ends run a backend-for-frontend with the same routes, cookies and rules, written as framework-free
            TypeScript and wired in with a few lines. Pick the one your team knows; the API doesn&apos;t change.
          </p>
        }
      />

      <div className="mt-12 grid items-start gap-x-16 gap-y-10 lg:mt-16 xl:grid-cols-12">
        <DiagramFigure spec={crmFrontEnds} name="crm-front-ends" className="xl:col-span-6" />
        <div className="min-w-0 xl:col-span-6">
          <h3 className="font-display text-display-sm font-semibold text-fg">The contract</h3>
          <Checks
            className="mt-4"
            items={[
              <>
                <Code>POST /api/auth/login</Code> signs in at Navree, keeps the tokens in httpOnly cookies (
                <Code>navree_at</Code>, <Code>navree_rt</Code>) and returns only the user.
              </>,
              <>Every other <Code>/api/*</Code> call is forwarded on the same path, so the API&apos;s OpenAPI document types the client.</>,
              <>A 401 refreshes once per refresh token, shared by every request in flight.</>,
              <>A write from another origin gets a 403 before it reaches Navree.</>,
              <><Code>/broadcasting/auth</Code> signs private channels for pusher-js.</>,
              <>The UI reads the access rules from <Code>GET /api/me/abilities</Code> with CASL.</>,
            ]}
          />
          <h3 className="mt-10 font-display text-display-sm font-semibold text-fg">The theme</h3>
          <p className="mt-3 max-w-2xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            Both are dark, on the navree.com tokens, with a sidebar that folds to icons with Ctrl B, a 48px top bar, a
            command palette on Ctrl K, 4px corners on controls and 6px on surfaces. shadcn/ui draws the React app and
            shadcn-vue the Vue one.
          </p>
        </div>
      </div>

      <div className="mt-12 grid gap-3 lg:grid-cols-2">
        {[
          { fe: next, source: NEXT_ROUTE },
          { fe: nuxt, source: NUXT_ROUTE },
        ].map(({ fe, source }) => (
          <div key={fe.id} className="flex min-w-0 flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5">
            <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 px-4 pt-4 pb-4 sm:px-5">
              <h3 className="text-lg font-semibold text-fg">{fe.name}</h3>
              <p className="font-mono text-xs text-fg-3">{fe.app}</p>
              <p className="w-full text-[0.9375rem] text-fg-2">{fe.stack}</p>
            </div>
            <CodePanel file={fe.proxyFile} lang="ts" source={source} wrap className="rounded-[0.875rem] [&_pre]:text-[0.75rem]!" />
          </div>
        ))}
      </div>
    </Section>
  );
}

/* ------------------------------------------------------- start your own */

function StartYourOwn() {
  return (
    <Section id="start">
      <SectionHeading
        split
        id="start-heading"
        eyebrow="Start your own"
        title="The same stack, one command away."
        lead={
          <p>
            The SaaS starter kit is built from the CRM&apos;s patterns: an API with sign-in, workspaces, billing and an admin
            panel, and a Next.js front end with the same BFF. The Nuxt front end is the CRM&apos;s own, on the{" "}
            <Code>@navree/nuxt</Code> module.
          </p>
        }
        action={<ArrowLink href="/docs/getting-started/starter-kits/">Read about the starter kits</ArrowLink>}
      />
      <div className="mt-12 grid gap-3 lg:mt-16 lg:grid-cols-2">
        <div className="flex min-w-0 flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel sm:p-7">
          <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">{frontEnds.next.name} front end</p>
          <h3 className="mt-2 font-display text-display-sm font-semibold text-fg">A SaaS app on Next.js</h3>
          <div className="mt-6 space-y-2">
            <CommandLine command="navree new acme --kit saas" />
            <CommandLine command="cd acme" />
            <CommandLine command="navree dev" />
          </div>
          <p className="mt-5 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            <Code>navree dev</Code> serves the API on :8080 and the web app on :3000. To run them apart, use{" "}
            <Code>navree dev --no-web</Code> and <Code>pnpm dev</Code> in <Code>web/</Code>.
          </p>
        </div>
        <div className="flex min-w-0 flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel sm:p-7">
          <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">{frontEnds.nuxt.name} front end</p>
          <h3 className="mt-2 font-display text-display-sm font-semibold text-fg">A Nuxt app on the same API</h3>
          <p className="mt-6 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            Put <Code>@navree/nuxt</Code> in the modules of <Code>nuxt.config.ts</Code> and Nitro serves the same BFF contract.
            The packages are in <Code>packages/js/</Code> of the repository, and the CRM&apos;s own Nuxt app is in{" "}
            <Code>{frontEnds.nuxt.app}</Code>. The{" "}
            <Link href="/docs/frontend/nuxt/" className="link">
              Nuxt guide
            </Link>{" "}
            shows the setup.
          </p>
        </div>
      </div>
    </Section>
  );
}

/* ---------------------------------------------------------- run this CRM */

const RUN_API = `git clone ${site.repoUrl}
cd navree/apps/crm-api
cargo run -p crm-api -- key:generate          # put it in .env as APP_KEY=...
cargo run -p crm-api -- migrate:fresh --seed  # SQLite: schema, roles, demo data
APP_REALTIME__ALLOWED_ORIGINS=http://localhost:3000,http://localhost:3102 \\
  cargo run -p crm-api -- serve               # http://127.0.0.1:8080`;

const RUN_NEXT = `cd navree/apps/crm-web
pnpm --dir ../../packages/js install && pnpm --dir ../../packages/js build   # once
pnpm install
pnpm dev                                      # http://localhost:3000`;

const RUN_NUXT = `cd navree/apps/crm-nuxt
pnpm packages                                 # once
pnpm install
NAVREE_API_URL=http://127.0.0.1:8080 NUXT_PUBLIC_NAVREE_REALTIME_WS_PORT=8080 pnpm dev
                                              # http://localhost:3102`;

const accounts = [
  { email: "admin@navree.test", role: "admin", can: "Everything, with users, roles and the audit log" },
  { email: "manager@navree.test", role: "manager", can: "Every record, reassign deals, export contacts" },
  { email: "rep1@navree.test", role: "rep", can: "Read everything, change only their own records" },
  { email: "rep2@navree.test", role: "rep", can: "A second rep, to watch live updates and ownership" },
];

function Run() {
  return (
    <Section id="run">
      <SectionHeading
        split
        id="run-heading"
        eyebrow="Run this CRM"
        title="Run it on your machine."
        lead={
          <p>
            You need Rust 1.95 or newer, Node.js and pnpm. No database server: development uses SQLite. Run each
            long-running command in its own terminal.
          </p>
        }
        action={<ArrowLink href="/docs/example-app/crm-tour/#running-it">Every option in the tour</ArrowLink>}
      />

      <ol className="mt-12 grid gap-3 lg:mt-16 lg:grid-cols-12">
        <li className="min-w-0 lg:col-span-12">
          <RunStep n={1} title="The API" text="Create a key, the database and its demo data, then serve the API and its WebSocket server.">
            <CodePanel file="apps/crm-api" lang="sh" source={RUN_API} />
          </RunStep>
        </li>
        <li className="min-w-0 lg:col-span-6">
          <RunStep n={2} title="The Next.js front end" text="Talks to the API at NAVREE_API_URL, http://127.0.0.1:8080 by default.">
            <CodePanel file="apps/crm-web" lang="sh" source={RUN_NEXT} />
          </RunStep>
        </li>
        <li className="min-w-0 lg:col-span-6">
          <RunStep n={2} title="Or the Nuxt front end" text="Point its BFF and its socket at the same API.">
            <CodePanel file="apps/crm-nuxt" lang="sh" source={RUN_NUXT} />
          </RunStep>
        </li>
      </ol>

      <div className="mt-3 grid gap-3 lg:grid-cols-12">
        <div className="overflow-hidden rounded-2xl border border-line bg-ink-1 lg:col-span-8">
          <table className="w-full text-left text-[0.875rem]">
            <caption className="border-b border-line px-5 py-4 text-left text-[0.9375rem] font-medium text-fg">
              Sign in with a demo account. Every password is <Code>password123</Code>.
            </caption>
            <thead className="sr-only">
              <tr>
                <th scope="col">Email</th>
                <th scope="col">Role</th>
                <th scope="col">Can</th>
              </tr>
            </thead>
            <tbody>
              {accounts.map((a) => (
                <tr key={a.email} className="border-t border-line first:border-t-0">
                  <td className="px-5 py-3 font-mono text-[0.8125rem] whitespace-nowrap text-fg-1">{a.email}</td>
                  <td className="hidden px-3 py-3 font-mono text-[0.8125rem] text-fg-3 sm:table-cell">{a.role}</td>
                  <td className="hidden px-5 py-3 text-fg-2 md:table-cell">{a.can}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="flex flex-col justify-between gap-6 rounded-2xl border border-line bg-ink-1 p-6 lg:col-span-4">
          <div>
            <h3 className="text-lg font-semibold text-fg">No Rust toolchain yet?</h3>
            <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
              <Code>pnpm dev:mock</Code> runs either front end against an in-browser mock of the API, with the same demo
              accounts.
            </p>
          </div>
          <ArrowLink href={`${site.repoUrl}/tree/${site.repoBranch}/apps`}>Browse the code on GitHub</ArrowLink>
        </div>
      </div>
    </Section>
  );
}

function RunStep({ n, title, text, children }: { n: number; title: string; text: string; children: ReactNode }) {
  return (
    <div className="flex h-full min-w-0 flex-col rounded-[1.25rem] border border-line bg-ink-1/60 p-1.5">
      <div className="flex gap-4 px-4 pt-4 pb-5 sm:px-5">
        <span className="font-mono text-xs text-accent numeric">{String(n).padStart(2, "0")}</span>
        <div className="min-w-0">
          <h3 className="text-base font-semibold text-fg">{title}</h3>
          <p className="mt-1 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{text}</p>
        </div>
      </div>
      <div className="[&_.code-block]:rounded-[0.875rem]">{children}</div>
    </div>
  );
}

export default function CrmShowcasePage() {
  return (
    <>
      <Hero />
      <Screens />
      <Built />
      <FrontEnds />
      <StartYourOwn />
      <Run />
      <PageSchema path="/showcase/crm/" metadata={metadata} />
    </>
  );
}
