import { changelog, commitUrl, formatDay } from "@/lib/changelog";
import { site } from "@site";

export const dynamic = "force-static";

const escape = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");

/** The newest days shown in the feed; the page holds the rest. */
const DAYS = 30;

/**
 * `/changelog/feed.xml`: the changelog as an Atom feed, one entry per day
 * with that day's changes in it, newest first. A reader or a crawler that
 * follows the feed learns about each deploy's changes without loading the page.
 */
export function GET() {
  const days = changelog().slice(0, DAYS);
  const updated = days[0]?.updated ?? new Date().toISOString();
  const entries = days
    .map((day) => {
      const items = day.changes
        .map(
          (change) =>
            `<li><strong>${change.type === "feat" ? "Feature" : "Fix"}${change.scope ? ` (${escape(change.scope)})` : ""}:</strong> ${escape(change.subject)} <a href="${commitUrl(change.hash)}">${change.hash.slice(0, 7)}</a></li>`,
        )
        .join("");
      return `  <entry>
    <id>${site.url}/changelog/#${day.day}</id>
    <title>${escape(`${site.name} changes on ${formatDay(day.day)}`)}</title>
    <link rel="alternate" type="text/html" href="${site.url}/changelog/#${day.day}"/>
    <updated>${day.updated}</updated>
    <summary>${escape(`${day.changes.length} ${day.changes.length === 1 ? "change" : "changes"}.`)}</summary>
    <content type="html">${escape(`<ul>${items}</ul>`)}</content>
  </entry>`;
    })
    .join("\n");
  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>${site.url}/changelog/</id>
  <title>${escape(`${site.name} changelog`)}</title>
  <subtitle>${escape(`Every new feature and fix in ${site.name}, by day.`)}</subtitle>
  <link rel="self" type="application/atom+xml" href="${site.url}/changelog/feed.xml"/>
  <link rel="alternate" type="text/html" href="${site.url}/changelog/"/>
  <updated>${updated}</updated>
  <author><name>${escape(site.name)}</name><uri>${site.url}</uri></author>
${entries}
</feed>
`;
  return new Response(xml, { headers: { "Content-Type": "application/atom+xml; charset=utf-8" } });
}
