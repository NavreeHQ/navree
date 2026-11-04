import { createElement, type ReactNode } from "react";

import { MARK_PATHS } from "@/components/site/logo";
import { picture } from "@/content/about";
import { cn } from "@/lib/utils";

import { FamilyRain } from "./family-rain";
import { pictureTree, type El } from "./picture-svg";

/** SVG attribute names as React props: `stroke-width` is `strokeWidth`, `class` is `className`. */
function propName(name: string): string {
  if (name === "class") return "className";
  if (name.startsWith("data-") || name.startsWith("aria-")) return name;
  return name.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());
}

/** The picture's element tree as React elements (children as arguments, so they need no keys). */
function toReact(node: El | string): ReactNode {
  if (typeof node === "string") return node;
  const props: Record<string, string | number> = {};
  for (const [name, value] of Object.entries(node.a ?? {})) if (value !== undefined) props[propName(name)] = value;
  return createElement(node.t, props, ...(node.c ?? []).map(toReact));
}

/**
 * The about picture: Amritpal at his desk with Navraj on his lap and Navreet
 * beside him, seen from behind and drawn in ones and zeros
 * (`picture-model.ts`). The SVG is the
 * whole picture without JavaScript and under reduced motion; `FamilyRain`
 * draws the falling digits over it while it is on screen.
 *
 * On phones the frame is wider than the screen and cropped, so the family
 * is drawn larger.
 */
export function FamilyPicture({ className }: { className?: string }) {
  return (
    <figure className={cn("min-w-0", className)}>
      <div className="max-sm:-mx-4 max-sm:overflow-hidden">
        <div className="group/picture @container relative max-sm:-ml-[5%] max-sm:w-[125%]">
          {toReact(pictureTree({ title: picture.title, desc: picture.desc, mark: MARK_PATHS }))}
          <FamilyRain className="pointer-events-none absolute inset-0 size-full" />
        </div>
      </div>
      <figcaption className="mt-3 text-sm leading-relaxed text-pretty text-fg-3 max-sm:mt-1">{picture.caption}</figcaption>
    </figure>
  );
}
