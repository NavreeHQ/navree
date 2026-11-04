import Link from "next/link";

import { PrimaryCta, SecondaryCta } from "@/components/home/parts";
import { GridPattern } from "@/components/magicui/grid-pattern";
import { showcaseImage } from "@/components/showcase/showcase-image";
import { asset, groups, kb, toolkitInfo } from "@/components/ui-docs/data";

/** The toolkit's own facts, counted from the UI catalogue and the toolkit's build. */
function facts() {
  const components = groups.reduce((n, g) => n + g.components.length, 0);
  return [
    { value: String(components), label: "components", text: `In ${groups.length} groups, each with a live preview` },
    { value: String(toolkitInfo.icons), label: "icons", text: "Only the ones you use go into your binary" },
    { value: kb(asset("core").gzip), label: "core runtime", text: "Gzip, measured from the build" },
    { value: "0", label: "scripts", text: "On a page with nothing interactive" },
    { value: "1", label: "stylesheet", text: "The same on every page" },
  ];
}

const srcSet = (list: { src: string; width: number }[]) => list.map((v) => `${v.src} ${v.width}w`).join(", ");

/**
 * The top of `/ui/`: what Navree UI is, the way in, and a whole app page drawn
 * by the toolkit: the shell, figures, a DataTable and a deal open in a drawer
 * with its history (the UI docs' `app-shell/full`). The picture is that page as
 * navree-ui draws it, captured by `scripts/showcase-images.ts`: 1440 px wide
 * from `md`, and at a phone's width below it, where the drawer covers the table.
 * A still rather than a live frame, so the first screen loads one picture and
 * nothing in it moves.
 */
export function UiHero() {
  const desktop = showcaseImage("hero-desktop");
  const phone = showcaseImage("hero-phone");
  const wide = "(min-width: 1280px) 784px, (min-width: 1024px) calc(100vw - 64px), calc(100vw - 48px)";
  return (
    <section aria-labelledby="ui-title" className="relative overflow-hidden">
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <GridPattern
          width={48}
          height={48}
          squares={[
            [22, 2],
            [25, 4],
            [19, 6],
            [28, 1],
          ]}
          className="[mask-image:radial-gradient(ellipse_70%_75%_at_78%_8%,black_10%,transparent_75%)] max-xl:[mask-image:radial-gradient(ellipse_120%_55%_at_50%_0%,black,transparent_80%)]"
        />
        <div className="hero-bloom absolute top-[-14rem] right-[-12rem] h-[52rem] w-[60rem] max-xl:top-auto max-xl:right-[-20rem] max-xl:bottom-[-10rem] max-xl:h-[36rem] max-xl:w-[44rem]" />
      </div>

      <div className="container-site relative grid items-center gap-x-12 gap-y-14 pt-14 pb-16 sm:pt-20 lg:pt-24 lg:pb-24 xl:grid-cols-12 xl:pt-28">
        <div className="min-w-0 xl:col-span-5">
          <p className="eyebrow">Navree UI</p>
          <h1 id="ui-title" className="mt-6 text-display-xl font-semibold text-balance text-fg">
            <span className="block">Your app&apos;s screens,</span>
            <span className="block text-fg-2">written in Rust.</span>
          </h1>
          <p className="mt-7 max-w-[36rem] text-lg leading-relaxed text-pretty text-fg-2">
            Navree UI is a toolkit of typed components that render to HTML on the server, in the same binary as your API. Every
            page gets one stylesheet, a small runtime that loads only what it uses, and a strict content security policy.
          </p>
          <div className="mt-10 flex flex-wrap items-center gap-3">
            <PrimaryCta href="#build-anything">See what you can build</PrimaryCta>
            <SecondaryCta href="/docs/ui/overview/">Get started</SecondaryCta>
          </div>
        </div>

        <figure className="min-w-0 xl:col-span-7">
          <div className="bezel shadow-glow-lg">
            {/* A picture of the real thing: the page navree-ui draws, at a laptop's width or a phone's. */}
            <div className="bezel-core overflow-hidden">
              <picture>
                <source
                  media="(min-width: 768px)"
                  type="image/avif"
                  srcSet={srcSet(desktop.avif)}
                  sizes={wide}
                  width={desktop.width}
                  height={desktop.height}
                />
                <source
                  media="(min-width: 768px)"
                  type="image/webp"
                  srcSet={srcSet(desktop.webp)}
                  sizes={wide}
                  width={desktop.width}
                  height={desktop.height}
                />
                <source type="image/avif" srcSet={srcSet(phone.avif)} sizes="calc(100vw - 48px)" width={phone.width} height={phone.height} />
                <source type="image/webp" srcSet={srcSet(phone.webp)} sizes="calc(100vw - 48px)" width={phone.width} height={phone.height} />
                {/* oxlint-disable-next-line next/no-img-element -- static export: art-directed sources by hand */}
                <img
                  src={(phone.webp[1] ?? phone.webp[0]).src}
                  width={phone.width}
                  height={phone.height}
                  alt={desktop.alt}
                  loading="eager"
                  fetchPriority="high"
                  className="block h-auto w-full"
                />
              </picture>
            </div>
          </div>
          <figcaption className="mt-3 px-1 text-sm leading-relaxed text-pretty text-fg-3">
            A sales team&apos;s deals: the app shell, four figures, a DataTable on the server and a deal open in a drawer with
            its history, rendered by <span className="font-mono text-[0.8125rem] text-fg-2">navree-ui</span> from the Rust
            in the UI docs.{" "}
            <Link href="/docs/ui/app-shell/#a-whole-page" prefetch={false} className="link">
              See it live
            </Link>
            .
          </figcaption>
        </figure>
      </div>

      <div className="relative border-t border-line bg-ink-0">
        <div className="container-site">
          {/* The 1px gaps show the line color through: crisp dividers at every column count. */}
          <dl className="grid grid-cols-2 gap-px bg-line lg:grid-cols-5">
            {facts().map((fact, i, all) => (
              <div
                key={fact.label}
                className={`flex flex-col gap-1.5 bg-ink-0 py-6 pr-4 sm:gap-2 sm:py-9 lg:pr-6 ${i % 2 === 1 ? "pl-5 sm:pl-8" : "pl-0"} ${
                  i === 0 ? "" : "lg:pl-8"
                } ${i === all.length - 1 ? "max-lg:col-span-2" : ""}`}
              >
                <dt className="order-2 text-[0.9375rem] leading-snug">
                  <span className="font-medium text-fg-1">{fact.label}</span>
                  <span className="mt-1 block text-sm text-balance text-fg-3 max-sm:hidden">{fact.text}</span>
                </dt>
                <dd className="order-1 font-display text-display-md font-semibold text-fg">{fact.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      </div>
    </section>
  );
}
