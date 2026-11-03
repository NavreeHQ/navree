import Link from "next/link";

import { HomeSection, SectionHeading } from "@/components/home/parts";
import { InlineMarkdown } from "@/components/inline-markdown";

/** What every page can do, each as its docs page states it. */
const features = [
  {
    title: "Forms and server validation",
    text: "One struct is the form. Its doc comments are the labels, its field types pick the widgets, and its `#[validate]` rules run in the browser and again on the server. `Submit<T>` reads it back, or answers with the errors under the inputs.",
    meta: "#[derive(Validate, Form)]",
    href: "/docs/ui/forms/",
  },
  {
    title: "Commands and ops",
    text: "A `Cmd` runs in the browser with no request: show, hide, open, focus. An `Op` is what the server sends back from an action: morph this element, show a toast, close that dialog. Both are data, and nothing is evaluated.",
    meta: "Cmd · Op · Reply",
    href: "/docs/ui/commands-and-ops/",
  },
  {
    title: "Regions and live updates",
    text: "A `Region` is a named part of a page with its own source. It loads after the page paints, polls while the page is visible, takes only what's new as a feed, or refetches when a reply says so, keeping focus and what was typed.",
    meta: "Region",
    href: "/docs/ui/regions/",
  },
  {
    title: "The app shell",
    text: "Every page in the same frame: a sidebar of grouped links that collapses with Ctrl+B and remembers it in a cookie, a top bar with the breadcrumb, and the page with its title and actions.",
    meta: "AppShell · AppPage",
    href: "/docs/ui/app-shell/",
  },
  {
    title: "Keyboard and accessibility",
    text: "Each component's page has its keys and its accessibility notes. Dialogs, accordions, popovers and switches are the browser's own elements, so they keep the browser's own keyboard behaviour, and a visible focus ring marks where you are.",
    meta: "<dialog> · <details> · popover",
    href: "/docs/components/overview/",
  },
  {
    title: "Right to left",
    text: "`Direction::rtl(content)` sets `dir=\"rtl\"` on a part of the page, and everything inside follows it: the text, the order of a row's items and the side a label sits on.",
    meta: "Direction",
    href: "/docs/components/direction/",
  },
];

/** "Built in": six short tiles, each linking to its docs page. */
export function UiFeatures() {
  return (
    <HomeSection id="built-in">
      <SectionHeading
        split
        id="built-in"
        eyebrow="Built in"
        title="What every page can do."
        lead={<p>The parts of a real app that sit between the components: input, actions, live data, the frame, the keyboard and the reading direction.</p>}
      />
      <ul className="mt-12 grid grid-cols-1 gap-3 md:grid-cols-2 lg:mt-14 xl:grid-cols-3">
        {features.map((f) => (
          <li key={f.title} className="min-w-0">
            <Link
              href={f.href}
              className="group flex h-full flex-col rounded-2xl border border-line bg-ink-1 p-6 shadow-bezel transition-colors duration-200 hover:border-line-2"
            >
              <h3 className="font-display text-display-sm font-semibold text-fg transition-colors group-hover:text-accent">{f.title}</h3>
              <p className="mt-3 text-[0.9375rem] leading-relaxed text-pretty text-fg-2">
                <InlineMarkdown text={f.text} />
              </p>
              <p className="mt-auto pt-5 font-mono text-xs text-fg-3">{f.meta}</p>
            </Link>
          </li>
        ))}
      </ul>
    </HomeSection>
  );
}
