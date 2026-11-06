import { changelog, commitUrl, formatDay } from "@/lib/changelog";
import { site } from "@site";

export const dynamic = "force-static";

const escape = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");

/** The newest weeks shown in the feed; the page holds the rest. */
const WEEKS = 12;

/**
 * `/changelog/feed.xml`: the changelog as an Atom feed, one entry per week
 * with that week's highlight, its releases and its changes by area in it,
 * newest first. A reader or a crawler that follows the feed learns about each
 * release without loading the page.
 */
export function GET() {
  const weeks = changelog().slice(0, WEEKS);
  const updated = weeks[0]?.updated ?? new Date().toISOString();
  const entries = weeks
    .map((week) => {
      const releases = week.releases
        .map((release) => `<li>${escape(`${site.name} ${release.version}`)}, <a href="${commitUrl(release.hash)}">${release.hash.slice(0, 7)}</a></li>`)
        .join("");
      const areas = week.areas
        .map((area) => {
          const items = area.changes
            .map(
              (change) =>
                `<li><strong>${change.type === "feat" ? "Feature" : "Fix"}${change.scope ? ` (${escape(change.scope)})` : ""}:</strong> ${escape(change.subject)} <a href="${site.url}${change.link.href}">${escape(change.link.label)}</a></li>`,
            )
            .join("");
          return `<h3>${escape(area.name)}</h3><ul>${items}</ul>`;
        })
        .join("");
      return `  <entry>
    <id>${site.url}/changelog/#week-${week.start}</id>
    <title>${escape(`${site.name} changes in the week of ${formatDay(week.start)}`)}</title>
    <link rel="alternate" type="text/html" href="${site.url}/changelog/#week-${week.start}"/>
    <updated>${week.updated}</updated>
    <summary>${escape(week.highlight)}</summary>
    <content type="html">${escape(`<p>${escape(week.highlight)}</p><ul>${releases}</ul>${areas}`)}</content>
  </entry>`;
    })
    .join("\n");
  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>${site.url}/changelog/</id>
  <title>${escape(`${site.name} changelog`)}</title>
  <subtitle>${escape(`Every ${site.name} release by week, newest first.`)}</subtitle>
  <link rel="self" type="application/atom+xml" href="${site.url}/changelog/feed.xml"/>
  <link rel="alternate" type="text/html" href="${site.url}/changelog/"/>
  <updated>${updated}</updated>
  <author><name>${escape(site.name)}</name><uri>${site.url}</uri></author>
${entries}
</feed>
`;
  return new Response(xml, { headers: { "Content-Type": "application/atom+xml; charset=utf-8" } });
}
