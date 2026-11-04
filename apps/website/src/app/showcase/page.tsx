import { ArrowRightIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";

import { ArrowLink, HomeSection, PrimaryCta, SecondaryCta, SectionHeading } from "@/components/home/parts";
import { CommandLine } from "@/components/showcase/code-panel";
import { frontEnds } from "@/components/showcase/crm-data";
import { BrowserFrame, PhoneFrame, ShotImage } from "@/components/showcase/frames";
import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";
import { entry } from "@/content/ecosystem";
import { pageMetadata } from "@/lib/metadata";
import { PageSchema } from "@/lib/structured-data";

import { apps } from "./apps";

export const metadata: Metadata = pageMetadata({
  title: "Showcase: apps and tools built on Navree",
  description:
    "Apps built on Navree, with code in the repository: the reference CRM with Next.js and Nuxt front ends, example apps in Navree UI, and tools such as Logbook.",
  path: "/showcase/",
});

/** The first-party tools with screens of their own, each a Navree module served by your app. */
const tools = ["lighthouse", "fleet", "helm", "logbook"].map(entry);
/** The tools whose pages are drawn with Navree UI (crates that depend on navree-ui). */
const drawnWithUi = new Set(["logbook"]);

const crmFacts = [
  { label: "API", value: "Rust, Navree" },
  { label: "Front ends", value: "Next.js · Nuxt" },
  { label: "Data", value: "Navree ORM on SQLite" },
  { label: "Live", value: "Pusher protocol" },
];

export default function ShowcasePage() {
  const next = frontEnds.next;
  const nuxt = frontEnds.nuxt;
  return (
    <>
      <section aria-labelledby="showcase-heading" className="relative isolate overflow-hidden">
        <div
          aria-hidden
          className="pointer-events-none absolute -top-40 right-[-10rem] -z-10 h-[40rem] w-[56rem] bg-[radial-gradient(ellipse_at_center,rgb(61_255_143/0.08),transparent_65%)]"
        />
        <div className="container-site pt-14 pb-12 sm:pt-20 lg:pt-28 lg:pb-16">
          <div className="max-w-3xl">
            <p className="eyebrow">Showcase</p>
            <h1 id="showcase-heading" className="mt-5 text-display-xl font-semibold text-balance text-fg">
              Real apps, built on Navree.
            </h1>
            <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-fg-2">
              Applications built with the framework and tested with it: a complete CRM, example apps drawn with Navree UI,
              and the first-party tools that run inside your own app. Their code lives in the repository, next to the
              crates they use.
            </p>
          </div>
        </div>
      </section>

      <section aria-labelledby="crm-card-heading" className="container-site pb-24 lg:pb-32">
        <article className="group relative overflow-hidden rounded-[1.5rem] border border-line bg-ink-1 shadow-bezel transition-colors duration-300 hover:border-line-2">
          <div className="grid lg:grid-cols-12">
            <div className="flex flex-col p-6 sm:p-8 lg:col-span-5 lg:p-10">
              <p className="font-mono text-xs tracking-[0.12em] text-fg-3 uppercase">Reference app</p>
              <h2 id="crm-card-heading" className="mt-3 text-display-lg font-semibold text-balance text-fg">
                <Link href="/showcase/crm/" className="after:absolute after:inset-0 after:content-['']">
                  Navree CRM
                </Link>
              </h2>
              <p className="mt-5 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
                Companies, contacts, a deal pipeline, tasks and a live dashboard for a sales team, with roles, an audit
                trail and real-time updates. One Navree API, two front ends.
              </p>
              <dl className="mt-8 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line">
                {crmFacts.map((f) => (
                  <div key={f.label} className="bg-ink-1 px-4 py-3">
                    <dt className="text-xs text-fg-3">{f.label}</dt>
                    <dd className="mt-1 text-[0.9375rem] font-medium text-fg-1">{f.value}</dd>
                  </div>
                ))}
              </dl>
              <span
                aria-hidden
                className="mt-8 inline-flex items-center gap-1.5 text-[0.9375rem] font-medium text-fg-1 transition-colors group-hover:text-accent lg:mt-auto lg:pt-10"
              >
                See the screens and how it is built
                <ArrowRightIcon className="size-4 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5" />
              </span>
            </div>

            <div className="relative min-w-0 border-line px-6 pb-6 sm:px-8 sm:pb-8 lg:col-span-7 lg:border-l lg:p-10 lg:pr-0">
              <div className="relative lg:-mr-24">
                <BrowserFrame address={`${next.origin}/deals`}>
                  <ShotImage
                    shot={next.shots.pipeline}
                    priority
                    sizes="(min-width: 1440px) 860px, (min-width: 1024px) 62vw, (min-width: 640px) calc(100vw - 112px), calc(100vw - 80px)"
                  />
                </BrowserFrame>
                <div className="absolute -bottom-4 left-6 hidden w-[9.5rem] sm:block lg:-bottom-6 lg:-left-10 lg:w-[11rem]">
                  <PhoneFrame className="shadow-float">
                    <ShotImage shot={nuxt.shots["phone-dashboard"]} sizes="176px" />
                  </PhoneFrame>
                </div>
              </div>
            </div>
          </div>
        </article>

        <div className="mt-3 grid gap-3 lg:grid-cols-12">
          <div className="flex flex-col justify-between gap-8 rounded-2xl border border-line bg-ink-1 p-6 sm:p-8 lg:col-span-7">
            <div>
              <h2 className="font-display text-display-sm font-semibold text-fg">Start from the same code</h2>
              <p className="mt-3 max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                The SaaS starter kit is built from the CRM&apos;s patterns: sign-in, workspaces, billing, an admin panel
                and a backend-for-frontend on Next.js. The CRM&apos;s Nuxt app shows the same contract on Nuxt.
              </p>
            </div>
            <div>
              <div className="grid gap-2">
                <CommandLine command="navree new acme --kit saas" />
              </div>
              <ArrowLink href="/docs/getting-started/starter-kits/" className="mt-6">
                Read about the starter kits
              </ArrowLink>
            </div>
          </div>
          <div className="flex flex-col justify-between gap-8 rounded-2xl border border-line bg-ink-1 p-6 sm:p-8 lg:col-span-5">
            <div>
              <h2 className="font-display text-display-sm font-semibold text-fg">Run the CRM</h2>
              <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                Clone the repository, seed SQLite with the demo team and sign in as a rep, a manager or an admin.
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <PrimaryCta href="/showcase/crm/#run">Run it locally</PrimaryCta>
              <SecondaryCta href="/docs/example-app/crm-tour/">Read the tour</SecondaryCta>
            </div>
          </div>
        </div>
      </section>

      <HomeSection id="apps-heading">
        <SectionHeading
          split
          id="apps-heading"
          eyebrow="Example apps"
          title="Products drawn with Navree UI, end to end."
          lead={
            <p>
              Each is a whole app on SQLite with seeded data and live updates, drawn by navree-ui from its own Rust and
              tested in three browsers. Every screen on its page is the app&apos;s own view.
            </p>
          }
          action={<ArrowLink href="/ui/#build-anything">See them on the UI page</ArrowLink>}
        />
        <ul className="mt-12 grid grid-cols-1 gap-3 lg:mt-14 lg:grid-cols-2">
          {apps.map((app) => {
            const preview = example(app.preview);
            return (
              <li key={app.slug} className="min-w-0">
                <article className="group relative flex h-full flex-col overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-200 hover:border-line-2">
                  <div className="border-b border-line" inert>
                    <PreviewFrame
                      src={preview.src}
                      title={app.previewTitle}
                      height={preview.height}
                      width={preview.width}
                      defer
                    />
                  </div>
                  <div className="flex flex-1 flex-col p-6 sm:p-8">
                    <h3 className="font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">
                      <Link href={`/showcase/${app.slug}/`} className="after:absolute after:inset-0 after:content-['']">
                        {app.name}
                      </Link>
                    </h3>
                    <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{app.summary}</p>
                    <dl className="mt-6 grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-line bg-line">
                      {app.facts.map((f) => (
                        <div key={f.label} className="bg-ink-1 px-4 py-3">
                          <dt className="text-xs text-fg-3">{f.label}</dt>
                          <dd className="mt-1 text-[0.9375rem] font-medium text-fg-1">{f.value}</dd>
                        </div>
                      ))}
                    </dl>
                    <span
                      aria-hidden
                      className="mt-auto inline-flex items-center gap-1.5 pt-6 text-[0.9375rem] font-medium text-fg-1 transition-colors group-hover:text-accent"
                    >
                      See the screens, the scale and how it is built
                      <ArrowRightIcon className="size-4 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5" />
                    </span>
                  </div>
                </article>
              </li>
            );
          })}
        </ul>
      </HomeSection>

      <HomeSection id="tools-heading">
        <SectionHeading
          split
          id="tools-heading"
          eyebrow="Also built on Navree"
          title="The tools that run your app are Navree modules too."
          lead={
            <p>
              Monitoring, queues, admin and debugging ship as modules of the framework, with their own routes, screens and
              tests, served by the app you build. Logbook&apos;s pages are drawn with{" "}
              <Link href="/ui/" className="link">
                Navree UI
              </Link>
              .
            </p>
          }
          action={<ArrowLink href="/ecosystem/">Every tool and package</ArrowLink>}
        />
        <ul className="mt-12 grid grid-cols-1 gap-3 md:grid-cols-2 lg:mt-14 lg:grid-cols-4">
          {tools.map((t) => (
            <li key={t.slug} className="min-w-0">
              <Link
                href={t.href}
                className="group flex h-full flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel transition-colors duration-200 hover:border-line-2"
              >
                <h3 className="font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">
                  {t.name}
                </h3>
                <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{t.tagline}.</p>
                <p className="mt-auto flex flex-wrap items-center gap-x-3 gap-y-1 pt-5 font-mono text-xs text-fg-3">
                  <span>{t.mount}</span>
                  {drawnWithUi.has(t.slug) ? <span className="font-sans text-fg-2">Drawn with Navree UI</span> : null}
                </p>
              </Link>
            </li>
          ))}
        </ul>
      </HomeSection>
      <PageSchema path="/showcase/" metadata={metadata} />
    </>
  );
}
