import { PrimaryCta, SecondaryCta } from "@/components/home/parts";
import { FlickeringGrid } from "@/components/magicui/flickering-grid";
import { highlight } from "@/lib/mdx";

/** The module list from the Navree UI overview page: the one line that turns the toolkit on. */
const SETUP = `Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(HttpModule::from_config())
    .module(UiModule::from_config()) // [!code highlight]
    .run()`;

/** The closing call to action: how to start, and where to read on. */
export async function UiClosing() {
  const html = (await highlight(SETUP, "rust")).replace(/^<pre[^>]*>/, "").replace(/<\/pre>\s*$/, "");
  return (
    <section aria-labelledby="ui-start" className="cv-auto relative overflow-hidden border-t border-line">
      <div aria-hidden className="pointer-events-none absolute inset-0">
        <FlickeringGrid
          squareSize={3}
          gridGap={6}
          flickerChance={0.18}
          maxOpacity={0.22}
          className="size-full [mask-image:radial-gradient(ellipse_55%_70%_at_50%_100%,black,transparent_75%)]"
        />
      </div>

      <div className="container-site section-y relative flex flex-col items-center text-center">
        <p className="eyebrow">Get started</p>
        <h2 id="ui-start" className="mt-5 max-w-[18ch] text-display-xl font-semibold text-balance text-fg">
          Start with one page.
        </h2>
        <p className="mt-6 max-w-xl text-lg leading-relaxed text-pretty text-fg-2">
          Navree UI is the <code className="inline-code">ui</code> feature of <code className="inline-code">navree</code>, on by
          default. Add its module to your app and return components from a handler.
        </p>

        <figure className="code-panel mt-10 w-full max-w-2xl text-left">
          <figcaption className="code-block-bar">
            <span className="code-block-title">src/main.rs</span>
          </figcaption>
          <pre
            className="shiki navree-night scroll-thin"
            tabIndex={0}
            // Shiki's output for a snippet in this file, highlighted at build time
            // (Shiki escapes the code it highlights).
            // nosemgrep: navree-raw-html
            dangerouslySetInnerHTML={{ __html: html }}
          />
        </figure>

        <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
          <PrimaryCta href="/docs/ui/overview/">Read the UI docs</PrimaryCta>
          <SecondaryCta href="/docs/components/overview/">Every component</SecondaryCta>
        </div>
      </div>
    </section>
  );
}
