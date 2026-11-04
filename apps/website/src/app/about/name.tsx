import { HomeSection } from "@/components/home/parts";
import { founder } from "@/content/about";

/** Where the name comes from: Navreet, without her last letter. */
export function TheName() {
  return (
    <HomeSection id="the-name">
      <div className="grid items-center gap-x-12 gap-y-12 lg:grid-cols-12">
        <div className="lg:col-span-6">
          <p className="eyebrow">The name</p>
          <h2 id="the-name" className="mt-5 text-display-lg font-semibold text-balance text-fg">
            Navree, from Navreet.
          </h2>
          <div className="mt-6 max-w-xl space-y-5 text-lg leading-relaxed text-pretty text-fg-2">
            <p>
              Navree takes its name from {founder.short}&apos;s daughter, Navreet: it is her name without its last letter.
              In the picture above, she is the one standing beside the chair with her braids down her back, watching the
              screen and learning what her father builds.
            </p>
            <p>
              Her younger brother, Navraj, is there too, sitting on his father&apos;s lap and facing the same screen.
            </p>
          </div>
        </div>

        {/* The name drawn as type: the last letter of "Navreet" stands apart, as an outline. */}
        <div aria-hidden className="@container min-w-0 lg:col-span-6">
          <div className="relative flex flex-col items-start lg:items-end">
            {/* Sized to its column (about 3.8 em wide), so it never reaches the text beside it. */}
            <p className="font-display text-[clamp(3.5rem,25cqi,10.5rem)] leading-[0.9] font-semibold tracking-[-0.045em] text-fg">
              Navree
              <span className="text-transparent [-webkit-text-stroke:1px_var(--color-fg-3)] [paint-order:stroke]">t</span>
            </p>
            <p className="mt-5 font-mono text-[0.8125rem] text-fg-3">
              <span className="text-fg-2">Navreet</span>
              <span className="px-2 text-accent">−</span>t<span className="px-2 text-accent">=</span>
              <span className="text-fg-2">Navree</span>
            </p>
          </div>
        </div>
      </div>
    </HomeSection>
  );
}
