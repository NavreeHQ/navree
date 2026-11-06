import { describe, expect, it } from "vitest";
import { contactsExportPath } from "@/features/contacts/queries";
import { createApiClient, unwrap } from "@/lib/api/client";

const BASE = "http://crm.test";

const page = { data: [], meta: { current_page: 1, per_page: 25, total: 0, last_page: 1 } };

/** A client that records the requests it sends and answers every one with an empty page. */
function recordingClient() {
  const requests: Request[] = [];
  const client = createApiClient({
    baseUrl: BASE,
    fetch: async (request) => {
      requests.push(request.clone());
      return new Response(JSON.stringify(page), { status: 200, headers: { "Content-Type": "application/json" } });
    },
  });
  return { client, requests };
}

/** The query of the one request that was sent, as a plain object. */
function sentQuery(requests: Request[]): Record<string, string> {
  expect(requests).toHaveLength(1);
  return Object.fromEntries(new URL(requests[0]!.url).searchParams);
}

describe("list requests", () => {
  it("send every filter as filter[name]=value, never as a flat parameter", async () => {
    const { client, requests } = recordingClient();

    await unwrap(
      client.GET("/api/v1/deals", {
        params: {
          query: {
            filter: { stage: "won", owner_id: undefined, company_id: "c1" },
            search: "Sutlej",
            sort: "-amount_cents",
            page: 2,
            per_page: 25,
          },
        },
      }),
    );

    expect(sentQuery(requests)).toEqual({
      "filter[stage]": "won",
      "filter[company_id]": "c1",
      search: "Sutlej",
      sort: "-amount_cents",
      page: "2",
      per_page: "25",
    });
  });

  it("send a boolean filter as text and leave a list without filters plain", async () => {
    const { client, requests } = recordingClient();

    await unwrap(
      client.GET("/api/v1/activities", {
        params: { query: { filter: { kind: "task", due: "overdue", completed: false }, per_page: 10 } },
      }),
    );
    expect(sentQuery(requests)).toEqual({
      "filter[kind]": "task",
      "filter[due]": "overdue",
      "filter[completed]": "false",
      per_page: "10",
    });

    const second = recordingClient();
    await unwrap(second.client.GET("/api/v1/users", { params: { query: { search: "Gill", per_page: 100 } } }));
    expect(sentQuery(second.requests)).toEqual({ search: "Gill", per_page: "100" });
  });

  it("write the contact export with the same filters and leave blank ones out", () => {
    expect(
      contactsExportPath({ filter: { status: "lead", owner_id: undefined, company_id: "" }, search: "Nair" }),
    ).toBe("/api/v1/contacts/export?filter%5Bstatus%5D=lead&search=Nair");
    expect(contactsExportPath({})).toBe("/api/v1/contacts/export");
  });
});
