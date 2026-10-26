import { createCn } from "cn/config";

/**
 * clsx + Tailwind conflict resolution, taught the Navree type scale so a size
 * (`text-title`, `text-figure`, `text-display-*`, `text-eyebrow`) and a color
 * token (`text-fg`, `text-accent`) on the same element both survive.
 */
export const cn = createCn({
  extend: {
    classGroups: {
      "font-size": [{ text: ["title", "figure", "eyebrow", "display-sm", "display-md", "display-lg"] }],
    },
  },
});
