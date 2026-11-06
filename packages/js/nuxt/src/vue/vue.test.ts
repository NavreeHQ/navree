import { NavreeError, type Operation } from "@navree/client";
import { createMemoryRealtime } from "@navree/client/realtime";
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { defineComponent, h, nextTick, ref } from "vue";
import { createMemoryHistory, createRouter } from "vue-router";
import { Can, installAbilities, provideAbilityReady, useAbility, useAbilityReady } from "./abilities.js";
import { createNavreeAuth } from "./auth.js";
import { applyFieldErrors } from "./forms.js";
import { mergeListQuery, useListQuery } from "./list-query.js";
import { useOperation } from "./operation.js";
import { createRealtimeController, installRealtime, useChannel, useRealtimeState } from "./realtime.js";
import { createWorkspace } from "./workspace.js";

const user = { __type: "User", id: "u1", name: "Ada" };

describe("createNavreeAuth", () => {
  function api(routes: Record<string, () => Response>) {
    return vi.fn(async (path: string, init?: RequestInit) => {
      const route = routes[`${init?.method ?? "GET"} ${path}`];
      return route ? route() : new Response(null, { status: 404 });
    });
  }

  it("signs in, loads the user and signs out", async () => {
    const onSignedOut = vi.fn();
    const fetch = api({
      "POST /api/auth/login": () => Response.json({ user }),
      "GET /api/auth/me": () => Response.json(user),
      "POST /api/auth/logout": () => new Response(null, { status: 204 }),
    });
    const auth = createNavreeAuth<typeof user>({ fetch, onSignedOut });
    expect(auth.status.value).toBe("unknown");

    expect(await auth.signIn({ email: "a@x.test", password: "pw" })).toEqual({ twoFactor: false, user });
    expect(auth.signedIn.value).toBe(true);
    expect(JSON.parse(fetch.mock.calls[0]![1]!.body as string)).toEqual({ email: "a@x.test", password: "pw" });

    expect(await auth.load()).toEqual(user);
    await auth.signOut({ all: true });
    expect(JSON.parse(fetch.mock.calls[2]![1]!.body as string)).toEqual({ all: true });
    expect(auth.user.value).toBeNull();
    expect(auth.status.value).toBe("signed-out");
    expect(onSignedOut).toHaveBeenCalledTimes(1);
  });

  it("asks for the two-factor code, then completes the sign-in", async () => {
    const fetch = api({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 120 }),
      "POST /api/auth/2fa/verify": () => Response.json({ user }),
    });
    const auth = createNavreeAuth({ fetch });
    expect(await auth.signIn({ email: "a", password: "b" })).toEqual({ twoFactor: true, expiresIn: 120 });
    expect(auth.status.value).toBe("two-factor");
    expect(await auth.verifyTwoFactor({ code: "123456" })).toEqual(user);
    expect(auth.status.value).toBe("signed-in");
  });

  it("throws the API's error for wrong credentials, and treats a 401 on load as signed out", async () => {
    const fetch = api({
      "POST /api/auth/login": () =>
        Response.json({ statusCode: 401, message: "Invalid credentials", error: "Unauthorized", code: "invalid_credentials" }, { status: 401 }),
      "GET /api/auth/me": () => new Response(null, { status: 401 }),
    });
    const auth = createNavreeAuth({ fetch });
    const error = await auth.signIn({ email: "a", password: "b" }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(NavreeError);
    expect((error as NavreeError).code).toBe("invalid_credentials");
    expect(await auth.load()).toBeNull();
    expect(auth.status.value).toBe("signed-out");
  });
});

describe("createWorkspace", () => {
  it("reads and writes the workspace cookie, then reports the switch", () => {
    const cookie = ref<string | null | undefined>("acme");
    const onChange = vi.fn();
    const workspace = createWorkspace(cookie, onChange);
    expect(workspace.workspace.value).toBe("acme");
    workspace.setWorkspace("globex");
    expect(cookie.value).toBe("globex");
    expect(onChange).toHaveBeenCalledWith("globex");
    workspace.setWorkspace(undefined);
    expect(cookie.value).toBeNull();
    expect(workspace.workspace.value).toBeUndefined();
  });
});

describe("useOperation", () => {
  const op = (status: Operation["status"], done: number): Operation => ({
    id: "op1",
    kind: "contacts.import",
    status,
    progress: { done, total: 4, percent: done * 25, note: null },
    result: null,
    error: null,
    owner_id: "u1",
    workspace_id: null,
    batch_id: null,
    created_at: "",
    started_at: null,
    heartbeat_at: null,
    finished_at: null,
    updated_at: "",
  });
  const sse = (events: Operation[]) =>
    new Response(events.map((e, i) => `event: operation\nid: ${i}\ndata: ${JSON.stringify(e)}\n\n`).join(""));

  it("follows the id until the operation finishes, and stops on unmount", async () => {
    const id = ref<string | null>(null);
    let signal: AbortSignal | undefined;
    const fetch = vi.fn(async (_path: string, init?: RequestInit) => {
      signal = init?.signal ?? undefined;
      return sse([op("running", 1), op("succeeded", 4)]);
    });
    const onFinished = vi.fn();
    const Progress = defineComponent({
      setup() {
        const { operation, finished } = useOperation(id, { fetch, onFinished });
        return () => h("p", `${operation.value?.status ?? "waiting"} ${finished.value}`);
      },
    });
    const wrapper = mount(Progress);
    expect(wrapper.text()).toBe("waiting false");
    expect(fetch).not.toHaveBeenCalled();

    id.value = "op1";
    await vi.waitFor(() => expect(wrapper.text()).toBe("succeeded true"));
    expect(onFinished).toHaveBeenCalledWith(expect.objectContaining({ status: "succeeded" }));
    wrapper.unmount();
    expect(signal?.aborted).toBe(true);
  });

  it("cancels", async () => {
    const fetch = vi.fn(async (_path: string, init?: RequestInit) =>
      init?.method === "DELETE" ? Response.json(op("cancelled", 1)) : sse([op("running", 1)]),
    );
    let cancel!: () => Promise<void>;
    const Progress = defineComponent({
      setup() {
        const result = useOperation("op1", { fetch });
        cancel = result.cancel;
        return () => h("p", result.operation.value?.status ?? "waiting");
      },
    });
    const wrapper = mount(Progress);
    await cancel();
    await nextTick();
    expect(wrapper.text()).toBe("cancelled");
    expect(fetch).toHaveBeenCalledWith("/api/operations/op1", expect.objectContaining({ method: "DELETE" }));
    wrapper.unmount();
  });
});

describe("useListQuery", () => {
  async function setup(path: string) {
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/:p(.*)*", component: { render: () => null } }] });
    await router.push(path);
    let state!: ReturnType<typeof useListQuery>;
    const List = defineComponent({
      setup() {
        state = useListQuery({ per_page: 25 });
        return () => h("p", JSON.stringify(state.query.value));
      },
    });
    mount(List, { global: { plugins: [router] } });
    return { router, state: () => state };
  }

  it("reads the list query from the route over the defaults", async () => {
    const { state } = await setup("/deals?filter[stage]=won&sort=-amount&page=2&tab=open");
    expect(state().query.value).toEqual({ filter: { stage: "won" }, sort: ["-amount"], page: 2, per_page: 25 });
  });

  it("writes filters, search, sort and page back, keeping the route's other parameters", async () => {
    const { router, state } = await setup("/deals?tab=open&page=3");
    await state().setFilter("stage", "won");
    expect(router.currentRoute.value.fullPath).toBe("/deals?tab=open&filter[stage]=won");
    await state().setSearch("acme");
    expect(router.currentRoute.value.query).toEqual({ tab: "open", "filter[stage]": "won", search: "acme" });
    await state().toggleSort("name");
    expect(router.currentRoute.value.query.sort).toBe("name");
    await state().setPage(2);
    expect(router.currentRoute.value.query.page).toBe("2");
  });

  it("merges a list query into a route query and drops the defaults", () => {
    expect(mergeListQuery({ tab: "open", page: "2" }, { per_page: 25, search: "a" }, { per_page: 25 })).toEqual({ tab: "open", search: "a" });
  });

  it("replaces every list parameter, the cursor ones too, and writes the canonical form", () => {
    const merged = mergeListQuery(
      { tab: "open", cursor: "a", before: "b", limit: "5", filter: "1", q: "old", page: "2" },
      { limit: 10, filter: { stage: { in: ["lead", "won"] } }, search: "acme corp" },
    );
    expect(merged).toEqual({ tab: "open", q: "old", "filter[stage][in]": "lead,won", search: "acme corp", limit: "10" });
    expect(Object.keys(merged)).toEqual(["tab", "q", "filter[stage][in]", "search", "limit"]);
  });

  it("reads the grammar's operators, null and cursor names, and a cursor takes no default page size", async () => {
    const { state } = await setup("/deals?filter[amount][gte]=100&filter[owner_id][null]=1&before=abc&limit=20");
    expect(state().query.value).toEqual({
      filter: { amount: { gte: "100" }, owner_id: { null: true } },
      before: "abc",
      limit: 20,
    });
    expect(state().problems.value).toEqual([]);
  });

  it("does not read names the grammar does not have", async () => {
    const { state } = await setup("/deals?q=acme&per=10&after=abc&f.stage=won&filters[stage]=won");
    expect(state().query.value).toEqual({ per_page: 25 });
    expect(state().problems.value).toEqual([]);
  });

  it("keeps what is right in an edited address and names what is not", async () => {
    const { state } = await setup("/deals?filter[stage]=won&filter[amount][regex]=1&page=0");
    expect(state().query.value).toEqual({ filter: { stage: "won" }, per_page: 25 });
    expect(state().problems.value.map((problem) => problem.param)).toEqual(["filter[amount][regex]", "page"]);
  });

  it("goes back to the first page of a cursor list when a filter, the search or the sort changes", async () => {
    const { router, state } = await setup("/deals?cursor=abc&before=def&limit=20&tab=open");
    await state().setFilter("stage", "won");
    expect(router.currentRoute.value.query).toEqual({ tab: "open", "filter[stage]": "won", limit: "20" });
    await state().setSearch("acme");
    expect(router.currentRoute.value.query).toEqual({ tab: "open", "filter[stage]": "won", search: "acme", limit: "20" });
    await state().toggleSort("name");
    expect(router.currentRoute.value.query).toEqual({ tab: "open", "filter[stage]": "won", search: "acme", sort: "name", limit: "20" });
  });

  it("writes an operator filter and a list as `in`", async () => {
    const { router, state } = await setup("/deals");
    await state().setFilter("amount", { gte: 100, lte: 900 });
    expect(router.currentRoute.value.query).toEqual({ "filter[amount][gte]": "100", "filter[amount][lte]": "900" });
    await state().setFilter("stage", ["lead", "won"]);
    expect(router.currentRoute.value.query).toEqual({
      "filter[amount][gte]": "100",
      "filter[amount][lte]": "900",
      "filter[stage][in]": "lead,won",
    });
  });
});

describe("applyFieldErrors (vee-validate)", () => {
  it("sets field errors and hands other messages on", () => {
    const error = NavreeError.fromResponse(new Response(null, { status: 400 }), {
      statusCode: 400,
      message: ["email is invalid", "plan is unknown"],
      error: "Bad Request",
      errors: { email: ["email is invalid"], plan: ["plan is unknown"] },
    });
    const setFieldError = vi.fn();
    const onOther = vi.fn();
    expect(applyFieldErrors(error, setFieldError, { fields: ["email"], onOther })).toBe(true);
    expect(setFieldError).toHaveBeenCalledWith("email", "email is invalid");
    expect(onOther).toHaveBeenCalledWith("plan is unknown");
    expect(applyFieldErrors(new Error("x"), setFieldError)).toBe(false);
  });

  it("renames the API's fields to the form's when fields is an object, and hands the rest on", () => {
    const error = NavreeError.fromResponse(new Response(null, { status: 400 }), {
      statusCode: 400,
      message: ["company_id is required", "stage is unknown", "constructor is odd"],
      error: "Bad Request",
      errors: { company_id: ["company_id is required"], stage: ["stage is unknown"], constructor: ["constructor is odd"] },
    });
    const setFieldError = vi.fn();
    const onOther = vi.fn();

    expect(applyFieldErrors(error, setFieldError, { fields: { company_id: "company", stage: "stage" }, onOther })).toBe(true);

    expect(setFieldError.mock.calls).toEqual([
      ["company", "company_id is required"],
      ["stage", "stage is unknown"],
    ]);
    // A name that only exists on Object.prototype is not a field of the form.
    expect(onOther).toHaveBeenCalledExactlyOnceWith("constructor is odd");
  });

  it("without fields, sets every field under the name the API used", () => {
    const error = NavreeError.fromResponse(new Response(null, { status: 400 }), {
      statusCode: 400,
      message: ["a", "b"],
      error: "Bad Request",
      errors: { a: ["a"], b: ["b"] },
    });
    const setFieldError = vi.fn();
    applyFieldErrors(error, setFieldError);
    expect(setFieldError.mock.calls).toEqual([["a", "a"], ["b", "b"]]);
  });
});

describe("real time", () => {
  it("delivers a channel's events while the component lives", async () => {
    const realtime = createMemoryRealtime();
    const controller = createRealtimeController();
    const seen: unknown[] = [];
    const channel = ref<string | null>("crm.deals");
    const Feed = defineComponent({
      setup() {
        useChannel(channel, "deal.updated", (data) => seen.push(data));
        const state = useRealtimeState();
        return () => h("p", state.value);
      },
    });
    const wrapper = mount(Feed, { global: { plugins: [{ install: (app) => installRealtime(app, controller) }] } });
    await controller.connect(() => realtime);
    await nextTick();

    expect(wrapper.text()).toBe("connected");
    realtime.emit("crm.deals", "deal.updated", { id: "d1" });
    expect(seen).toEqual([{ id: "d1" }]);

    channel.value = "users.u1";
    await nextTick();
    expect(realtime.listening("crm.deals")).toBe(false);
    expect(realtime.listening("users.u1")).toBe(true);

    wrapper.unmount();
    expect(realtime.listening("users.u1")).toBe(false);
    controller.disconnect();
    expect(controller.state.value).toBe("disconnected");
  });

  it("keeps working when the connection fails", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const controller = createRealtimeController();
    await controller.connect(() => {
      throw new Error("no socket");
    });
    expect(controller.connection.value).toBeNull();
    expect(info).toHaveBeenCalled();
  });
});

describe("abilities", () => {
  it("installs one ability; rule updates re-render <Can> and useAbility users", async () => {
    let ability!: ReturnType<typeof installAbilities>;
    const View = defineComponent({
      setup() {
        const can = useAbility();
        return () =>
          h("div", [
            h(Can, { I: "update", this: { __type: "Deal", owner_id: "u1" } }, () => h("span", "edit")),
            h("span", can.can("delete", "Deal") ? "delete" : "read-only"),
          ]);
      },
    });
    const wrapper = mount(View, { global: { plugins: [{ install: (app) => (ability = installAbilities(app)) }] } });
    expect(wrapper.text()).toBe("read-only");

    ability.update([{ action: "update", subject: "Deal", conditions: { owner_id: "u1" } }, { action: "delete", subject: "Deal" }] as never);
    await flushPromises();
    expect(wrapper.text()).toBe("editdelete");
  });

  it("tells a shell when the rules are in: false while they load, true after; always true without abilities", async () => {
    let ready!: ReturnType<typeof provideAbilityReady>;
    const View = defineComponent({
      setup() {
        const flag = useAbilityReady();
        return () => h("p", flag.value ? "ready" : "loading");
      },
    });
    const wrapper = mount(View, { global: { plugins: [{ install: (app) => (ready = provideAbilityReady(app)) }] } });
    expect(wrapper.text()).toBe("loading");

    ready.value = true;
    await nextTick();
    expect(wrapper.text()).toBe("ready");

    ready.value = false;
    await nextTick();
    expect(wrapper.text()).toBe("loading");

    const without = mount(View);
    expect(without.text()).toBe("ready");
  });
});
