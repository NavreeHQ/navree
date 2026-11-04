import { ruleFor } from "@/components/architecture/rules";
import { ArrowLink, HomeSection, SectionHeading } from "@/components/home/parts";
import { founder, lessonId, lessons } from "@/content/about";

/** What the fifteen years taught, one row per lesson: the lesson on the left, the part of Navree it became on the right. */
export function Lessons() {
  return (
    <HomeSection id="lessons">
      <SectionHeading
        split
        id="lessons"
        eyebrow="What the years taught"
        title="Every lesson became a part of Navree."
        lead={
          <p>
            Six startups teach the same things more than once. These are the ones {founder.short} built into the framework,
            each with the page where you can see it at work.
          </p>
        }
      />

      <div className="mt-12 hidden grid-cols-12 gap-x-12 pb-4 font-mono text-eyebrow text-fg-3 uppercase lg:mt-16 lg:grid">
        <p className="col-span-6 pl-12">The lesson</p>
        <p className="col-span-6">In Navree</p>
      </div>
      <ol className="mt-10 border-t border-line lg:mt-0">
        {lessons.map((lesson, i) => {
          // The rule on /architecture/ that shows this lesson in the code, when the page shows it.
          const rule = ruleFor(lesson.rules);
          return (
            <li
              key={lesson.title}
              id={lessonId(lesson)}
              className="grid scroll-mt-(--header-h) gap-x-12 gap-y-5 border-b border-line py-8 lg:grid-cols-12 lg:py-10"
            >
              <div className="flex gap-5 lg:col-span-6">
                <span aria-hidden className="w-7 shrink-0 pt-1 font-mono text-sm text-fg-3 tabular-nums">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <div>
                  <h3 className="font-display text-display-sm font-semibold text-balance text-fg">{lesson.title}</h3>
                  <p className="mt-2.5 max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{lesson.lesson}</p>
                </div>
              </div>
              <div className="pl-12 lg:col-span-6 lg:pl-0">
                <p className="flex items-center gap-2.5 text-[0.9375rem] font-medium text-fg-1">
                  <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-accent shadow-glow-sm" />
                  <span className="text-fg-3 lg:sr-only">In Navree: </span>
                  {lesson.feature}
                </p>
                <p className="mt-2 max-w-xl text-[0.9375rem] leading-relaxed text-pretty text-fg-2">{lesson.text}</p>
                <div className="mt-3 flex flex-wrap gap-x-6 gap-y-2">
                  <ArrowLink href={lesson.link.href} className="text-sm">
                    {lesson.link.label}
                  </ArrowLink>
                  {rule ? (
                    <ArrowLink href={`/architecture/#${rule.id}`} className="text-sm">
                      The rule in the code
                    </ArrowLink>
                  ) : null}
                </div>
              </div>
            </li>
          );
        })}
      </ol>
    </HomeSection>
  );
}
