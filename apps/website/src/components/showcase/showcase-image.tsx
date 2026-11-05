import manifest from "../../../public/images/showcase/manifest.json";

import { cn } from "@/lib/utils";

/**
 * A picture of the example apps or of a toolkit component, as
 * `scripts/showcase-images.ts` captured it: AVIF where the browser takes it,
 * WebP otherwise, every width in `srcset`, and the size in `width` and
 * `height` so nothing moves while it loads. Lazy unless `priority`.
 */
export type ShowcaseImageData = {
  key: string;
  width: number;
  height: number;
  alt: string;
  /** The day it was captured (`2026-11-04`); the manifest's own date for a picture captured before this was kept. */
  captured?: string;
  avif: { src: string; width: number }[];
  webp: { src: string; width: number }[];
};

const images = new Map((manifest.images as ShowcaseImageData[]).map((image) => [image.key, image]));

/** The picture with this key; a key the manifest doesn't have fails the build. */
export function showcaseImage(key: string): ShowcaseImageData {
  const image = images.get(key);
  if (!image) throw new Error(`showcase image "${key}" is not in public/images/showcase/manifest.json; run scripts/showcase-images.ts`);
  return image;
}

/** The day the picture with this key was captured, as `2026-11-04`. */
export function showcaseCaptured(key: string): string {
  return showcaseImage(key).captured ?? manifest.captured;
}

const srcSet = (list: { src: string; width: number }[]) => list.map((v) => `${v.src} ${v.width}w`).join(", ");

export function ShowcaseImage({
  name,
  sizes,
  alt,
  priority = false,
  className,
}: {
  /** The key in the manifest. */
  name: string;
  /** How wide the picture is drawn at each breakpoint. */
  sizes: string;
  /** Overrides the manifest's alt text (`""` when a caption or a link already says it). */
  alt?: string;
  priority?: boolean;
  className?: string;
}) {
  const image = showcaseImage(name);
  const fallback = image.webp.find((v) => v.width >= 960) ?? image.webp.at(-1);
  return (
    <picture>
      <source type="image/avif" srcSet={srcSet(image.avif)} sizes={sizes} />
      <source type="image/webp" srcSet={srcSet(image.webp)} sizes={sizes} />
      {/* oxlint-disable-next-line next/no-img-element -- static export: sources and srcset by hand */}
      <img
        src={fallback?.src}
        width={image.width}
        height={image.height}
        alt={alt ?? image.alt}
        loading={priority ? "eager" : "lazy"}
        fetchPriority={priority ? "high" : undefined}
        decoding="async"
        className={cn("block h-auto w-full", className)}
      />
    </picture>
  );
}
