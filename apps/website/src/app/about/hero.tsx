import { FamilyPicture } from "@/components/about/family-picture";
import { ArrowLink } from "@/components/home/parts";
import { founder } from "@/content/about";

/** The top of `/about/`: who built Navree and whose name it carries, beside the family picture. */
export function AboutHero() {
  return (
    <section aria-labelledby="about-title" className="relative overflow-hidden">
      <div className="container-site relative grid items-center gap-x-12 gap-y-10 pt-12 pb-14 sm:pt-16 sm:gap-y-12 lg:pt-20 lg:pb-20 xl:grid-cols-12 xl:pt-24 xl:pb-24">
        <div className="min-w-0 xl:col-span-5">
          <p className="eyebrow">About</p>
          <h1 id="about-title" className="mt-6 text-display-xl font-semibold text-balance text-fg">
            <span className="block">Fifteen years of building,</span>
            <span className="block text-fg-2">named for Navreet.</span>
          </h1>
          <p className="mt-7 max-w-[36rem] text-lg leading-relaxed text-pretty text-fg-2">
            Navree is the work of {founder.name}, {founder.role} in {founder.place}. It holds what fifteen years and six
            startups taught him about building products, and it carries his daughter&apos;s name.
          </p>
          <div className="mt-9 flex flex-wrap items-center gap-x-8 gap-y-3">
            <ArrowLink href="#lessons">What the years taught</ArrowLink>
            <ArrowLink href="#contact">Write to {founder.short}</ArrowLink>
          </div>
        </div>

        <FamilyPicture className="xl:col-span-7" />
      </div>
    </section>
  );
}
