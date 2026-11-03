/**
 * The text of titles and descriptions as search results show it. No imports
 * and only erasable TypeScript: the page metadata (`metadata.ts`), the
 * check scripts and the tests all use these functions, and Node runs this
 * file directly.
 */

/** Search results show about this much of a title and a description; `scripts/check-seo.ts` enforces the same bounds. */
export const TITLE_MIN = 30;
export const TITLE_MAX = 60;
export const DESCRIPTION_MIN = 70;
export const DESCRIPTION_MAX = 160;

const oneLine = (text: string) => text.replace(/`/g, "").replace(/\s+/g, " ").trim();
const sentence = (text: string) => (/[.!?]$/.test(text) ? text : `${text}.`);

/**
 * A description that fits a search result: 70 to 160 characters.
 *
 * - Short enough already: kept as written. Under 70, the optional `context`
 *   (where the page sits, as a sentence) is added after it.
 * - Too long: whole sentences are kept while they fit. A single long
 *   sentence is cut at the last comma or semicolon that fits, at least 90
 *   characters in, and ends with an ellipsis, so it never claims to be
 *   complete; with no such place it is cut at a word.
 *
 * The page itself keeps the full text, and so does the structured data.
 */
export function metaDescription(text: string, context?: string): string {
  const full = oneLine(text);
  if (full.length <= DESCRIPTION_MAX) {
    if (full.length >= DESCRIPTION_MIN || !context) return full;
    const extended = `${sentence(full)} ${context}`;
    return extended.length <= DESCRIPTION_MAX ? extended : full;
  }

  // Whole sentences while they fit.
  const sentences = full.split(/(?<=[.!?])\s+(?=[A-Z0-9"(])/);
  let kept = "";
  for (const next of sentences) {
    const joined = kept ? `${kept} ${next}` : next;
    if (joined.length > DESCRIPTION_MAX) break;
    kept = joined;
  }
  if (kept.length >= DESCRIPTION_MIN) return kept;

  // One long sentence: the last clause break that fits and leaves enough text.
  let cut = -1;
  for (const match of full.matchAll(/[,;](?=\s)/g)) {
    const at = match.index ?? 0;
    if (at >= DESCRIPTION_MAX - 1) break;
    if (at >= 90) cut = at;
  }
  if (cut !== -1) return `${full.slice(0, cut).replace(/[,;:\s]+$/, "")}…`;
  const words = full.slice(0, DESCRIPTION_MAX - 1).replace(/\s+\S*$/, "");
  return `${words.replace(/[,;:.\s]+$/, "")}…`;
}

/**
 * The `<title>` of a docs page, 30 to 60 characters: the page's title, its
 * section and the documentation's name, in the longest of these forms that
 * fits. The `<h1>` stays the page's own title.
 *
 *   Validation | The basics | Navree documentation
 *   Validation | The basics | Navree docs
 *   Validation | Navree documentation
 *   Validation | Navree docs
 *   Validation | Navree
 */
export function docTitle(title: string, sectionTitle: string, siteName: string): string {
  const repeated = sectionTitle.length > 4 && title.toLowerCase().includes(sectionTitle.toLowerCase());
  const section = repeated ? null : sectionTitle;
  const forms = [
    section && `${title} | ${section} | ${siteName} documentation`,
    section && `${title} | ${section} | ${siteName} docs`,
    `${title} | ${siteName} documentation`,
    `${title} | ${siteName} docs`,
    `${title} | ${siteName}`,
  ].filter((form): form is string => Boolean(form));
  return forms.find((form) => form.length <= TITLE_MAX) ?? forms[forms.length - 1];
}
