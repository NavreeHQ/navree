import { DiagramFigure, diagrams } from "@/components/diagrams";
import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";

const stretches = [
  { title: "Read the design", text: "Copies it in and lists its screens, the states it lacks and the data it implies. You choose, screen by screen, to keep its look or translate it." },
  { title: "Map it to Navree UI", text: "Tokens, a spec per page, and every question, deviation and gap, each with a picture or an interim form." },
  { title: "Build and verify", text: "Builds the pages in Rust, then checks them beside the design, by eye and by tool." },
];

/** "Bring your design": the navree-design-to-ui skill, with its phase diagram. */
export function BringYourDesign() {
  return (
    <HomeSection id="bring-your-design">
      <div className="grid grid-cols-1 gap-x-12 gap-y-12 lg:grid-cols-12">
        <div className="min-w-0 lg:col-span-5">
          <SectionHeading
            id="bring-your-design"
            eyebrow="Bring your design"
            title="Turn a design into Navree UI pages."
            lead={
              <p>
                Have an HTML mock-up, a design-tool export or screenshots? The{" "}
                <code className="inline-code">navree-design-to-ui</code> agent skill turns them into working pages. Your coding
                agent does the work, and stops three times for your decision.
              </p>
            }
          />
          <ol className="mt-8">
            {stretches.map((s, i) => (
              <li key={s.title} className="flex gap-4 border-t border-line py-4">
                <span className="mt-0.5 font-mono text-xs text-fg-3 numeric">{String(i + 1).padStart(2, "0")}</span>
                <span>
                  <span className="block text-[0.9375rem] font-semibold text-fg">{s.title}</span>
                  <span className="mt-1 block text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{s.text}</span>
                </span>
              </li>
            ))}
          </ol>
          <div className="mt-6 flex flex-wrap gap-x-6 gap-y-3">
            <ArrowLink href="/docs/getting-started/bring-your-design/">Bring your design</ArrowLink>
            <ArrowLink href="/docs/getting-started/bring-your-product/">Or bring a whole product</ArrowLink>
          </div>
        </div>
        <div className="min-w-0 lg:col-span-7 lg:self-center">
          <DiagramFigure spec={diagrams["design-to-ui"]} name="design-to-ui" maxScale={1.15} />
        </div>
      </div>
    </HomeSection>
  );
}
