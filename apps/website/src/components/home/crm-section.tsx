import { frontEnds } from "@/components/showcase/crm-data";
import { BrowserFrame, PhoneFrame, ShotImage } from "@/components/showcase/frames";

import { ArrowLink } from "./parts";

const points = [
  { title: "Rust API", text: "Feature crates on one core, access rules in one function, events, queues and real time." },
  { title: "Two front ends", text: "Next.js and Nuxt, each with a BFF that keeps the tokens in httpOnly cookies." },
  { title: "Runs locally", text: "SQLite and a seeded sales team: sign in as a rep, a manager or an admin." },
];

/**
 * "A real CRM, built on Navree": the reference app, one large screenshot of
 * its pipeline, and the way to the showcase. A block of the homepage's apps
 * section (`apps-section.tsx`), under its heading.
 */
export function CrmBlock() {
  const next = frontEnds.next;
  return (
    <div>
      <div className="grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-12">
        <h3 className="font-display text-display-md font-semibold text-balance text-fg lg:col-span-7">A real CRM, built on Navree.</h3>
        <div className="lg:col-span-5 lg:pb-1">
          <p className="max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
            Companies, contacts, a deal pipeline, tasks and a live dashboard, in the repository and tested with the framework.
            The code in the section above comes from it.
          </p>
          <ArrowLink href="/showcase/crm/" className="mt-4">
            Explore the CRM
          </ArrowLink>
        </div>
      </div>

      <div className="relative mt-10 lg:mt-12 lg:pr-20 xl:pr-28">
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 -top-24 -z-10 mx-auto h-[36rem] max-w-[72rem] bg-[radial-gradient(ellipse_at_center,rgb(61_255_143/0.09),transparent_62%)]"
        />
        <BrowserFrame
          address={`${next.origin}/deals`}
          bar={<span className="hidden font-mono text-xs text-fg-3 sm:inline">{next.app}</span>}
        >
          <ShotImage
            shot={next.shots.pipeline}
            sizes="(min-width: 1440px) 1270px, (min-width: 1024px) calc(100vw - 170px), (min-width: 640px) calc(100vw - 64px), calc(100vw - 48px)"
          />
        </BrowserFrame>
        <div className="absolute right-0 -bottom-10 hidden w-[14rem] lg:block xl:w-[15.5rem]">
          <PhoneFrame className="shadow-float">
            <ShotImage shot={frontEnds.nuxt.shots["phone-deal"]} sizes="248px" />
          </PhoneFrame>
        </div>
      </div>

      <ul className="mt-12 grid gap-x-10 gap-y-6 sm:grid-cols-3 lg:mt-20 lg:max-w-[calc(100%-8rem)]">
        {points.map((p) => (
          <li key={p.title} className="border-t border-line pt-5">
            <h4 className="text-[0.9375rem] font-semibold text-fg">{p.title}</h4>
            <p className="mt-2 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{p.text}</p>
          </li>
        ))}
      </ul>
    </div>
  );
}
