import { example } from "@/components/ui-docs/data";
import { PreviewFrame } from "@/components/ui-docs/preview-frame";

import { RichText } from "./rich-text";

/** The height a whole-page preview gets on a phone, where it is drawn at the phone's own width. */
const PHONE_HEIGHT = 720;

/**
 * What navree-ui drew from an example app's Rust (a ui-docs example), framed
 * at the full content width, with its caption.
 */
export function Preview({
  id,
  title,
  caption,
  interactive,
  defer,
}: {
  id: string;
  title: string;
  caption: string;
  interactive?: boolean;
  /** Load the frame only as it nears the screen, after the page has loaded. */
  defer?: boolean;
}) {
  const preview = example(id);
  return (
    <figure className="bezel min-w-0">
      <div className="bezel-core overflow-hidden" inert={!interactive || undefined}>
        <PreviewFrame
          src={preview.src}
          title={title}
          height={preview.height}
          width={preview.width}
          phoneHeight={PHONE_HEIGHT}
          defer={defer}
        />
      </div>
      <figcaption className="px-3 pt-3 pb-1.5 text-sm leading-relaxed text-pretty text-fg-3">
        <RichText text={caption} />
      </figcaption>
    </figure>
  );
}
