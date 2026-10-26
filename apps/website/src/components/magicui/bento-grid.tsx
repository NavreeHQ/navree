import { ArrowRightIcon } from "lucide-react";
import Link from "next/link";
import type { ComponentPropsWithoutRef, ElementType, ReactNode } from "react";

import { cn } from "@/lib/utils";

interface BentoGridProps extends ComponentPropsWithoutRef<"div"> {
  children: ReactNode;
}

interface BentoCardProps extends Omit<ComponentPropsWithoutRef<"div">, "title"> {
  name: string;
  description: ReactNode;
  /**
   * A product mock, a diagram or a pattern. It fills the card behind the
   * text (absolutely positioned); keep the bottom clear or mask it, e.g.
   * `[mask-image:linear-gradient(to_bottom,black_40%,transparent)]`.
   */
  background?: ReactNode;
  Icon?: ElementType<{ className?: string; "aria-hidden"?: boolean }>;
  href?: string;
  cta?: string;
  /** Heading level for the card name (default h3). */
  as?: "h2" | "h3" | "h4";
}

/**
 * An asymmetric grid of cards. One column on phones, six on wide screens:
 * give each card its span, e.g. `lg:col-span-4 lg:row-span-2` beside two
 * `lg:col-span-2` cards.
 */
function BentoGrid({ children, className, ...props }: BentoGridProps) {
  return (
    <div className={cn("grid w-full auto-rows-[minmax(18rem,auto)] grid-cols-1 gap-3 lg:grid-cols-6", className)} {...props}>
      {children}
    </div>
  );
}

function BentoCard({ name, className, background, Icon, description, href, cta, as: Heading = "h3", ...props }: BentoCardProps) {
  return (
    <div
      className={cn(
        "group relative flex flex-col justify-end overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel transition-colors duration-300 hover:border-line-2",
        className,
      )}
      {...props}
    >
      {background ? <div className="pointer-events-none absolute inset-0">{background}</div> : null}
      <div className="relative z-10 flex flex-col gap-2 p-6">
        {Icon ? <Icon aria-hidden className="mb-1 size-5 text-fg-2 transition-colors duration-300 group-hover:text-accent" /> : null}
        <Heading className="font-sans text-[1.0625rem] leading-snug font-semibold tracking-[-0.01em] text-fg">{name}</Heading>
        <div className="max-w-lg text-[0.9375rem] leading-relaxed text-fg-2">{description}</div>
        {href && cta ? (
          <Link
            href={href}
            className="mt-2 inline-flex w-fit items-center gap-1.5 rounded-md text-sm font-medium text-fg-1 transition-colors hover:text-accent"
          >
            {cta}
            <ArrowRightIcon aria-hidden className="size-3.5 transition-transform duration-300 ease-(--ease-out-quint) group-hover:translate-x-0.5" />
          </Link>
        ) : null}
      </div>
    </div>
  );
}

export { BentoCard, BentoGrid };
