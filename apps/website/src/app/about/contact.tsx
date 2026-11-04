import { ArrowRightIcon } from "lucide-react";

import { GitHubIcon } from "@/components/icons";
import { HomeSection } from "@/components/home/parts";
import { Button } from "@/components/ui/button";
import { founder, issues } from "@/content/about";

/** How to reach Amritpal: his email and his GitHub, and where reports about Navree itself go. */
export function Contact() {
  return (
    <HomeSection id="contact">
      <div className="grid items-end gap-x-12 gap-y-10 lg:grid-cols-12">
        <div className="lg:col-span-7">
          <p className="eyebrow">Get in touch</p>
          <h2 id="contact" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            Write to {founder.short}.
          </h2>
          <p className="mt-5 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
            A question about Navree, an idea for it, or a product you want to build with it: write to him directly.
          </p>
        </div>

        <div className="lg:col-span-5 lg:pb-1">
          <p className="font-mono text-[0.9375rem] break-all text-fg-1">{founder.email}</p>
          <div className="mt-5 flex flex-wrap items-center gap-3">
            <Button asChild size="lg" className="pr-1.5 pl-6">
              <a href={`mailto:${founder.email}`} className="group">
                Email {founder.short}
                <span
                  aria-hidden
                  className="inline-flex size-9 items-center justify-center rounded-full bg-accent-ink/10 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5"
                >
                  <ArrowRightIcon className="size-4" />
                </span>
              </a>
            </Button>
            <Button asChild size="lg" variant="outline" className="px-6">
              <a href={founder.github.url}>
                <GitHubIcon className="size-4" />
                {founder.github.handle}
              </a>
            </Button>
          </div>
        </div>
      </div>

      <p className="mt-12 max-w-2xl border-t border-line pt-6 text-sm leading-relaxed text-pretty text-fg-3">
        For a bug or a feature request in Navree itself,{" "}
        <a href={issues} className="link">
          open an issue
        </a>{" "}
        so everyone with the same question can follow the answer. Security reports go privately to{" "}
        <a href="mailto:security@navree.com" className="link">
          security@navree.com
        </a>
        .
      </p>
    </HomeSection>
  );
}
