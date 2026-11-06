/**
 * The login page mounted in a real Nuxt app (the @navree/nuxt module with its sign-in state and
 * abilities, plus the app's plugins: TanStack Query and the API events), talking to a stubbed BFF
 * through `fetch`. Navigation is recorded, not performed.
 */
import { mockNuxtImport, mountSuspended } from "@nuxt/test-utils/runtime";
import { flushPromises } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import LoginPage from "@/pages/login.vue";

const { navigateTo } = vi.hoisted(() => ({ navigateTo: vi.fn() }));
mockNuxtImport("navigateTo", () => navigateTo);

const user = { __type: "User", id: "u9", name: "Daljit Kaur", email: "admin@navree.test", roles: ["admin"] };
type Call = { route: string; body: Record<string, unknown> };

/**
 * The BFF as the browser sees it. Anything not scripted is a 401 (no session yet), except the
 * abilities the module loads after a sign-in, which are empty.
 */
function stubBff(scripted: Record<string, (body: Record<string, unknown>) => Response>): Call[] {
  const routes: Record<string, (body: Record<string, unknown>) => Response> = {
    "GET /api/me/abilities": () => Response.json([]),
    ...scripted,
  };
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: Request | string, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(new URL(input, window.location.origin), init);
      const route = `${request.method} ${new URL(request.url).pathname}`;
      const text = request.method === "GET" ? "" : await request.text();
      const body = text ? (JSON.parse(text) as Record<string, unknown>) : {};
      calls.push({ route, body });
      const handler = routes[route];
      return handler
        ? handler(body)
        : Response.json({ statusCode: 401, message: "Unauthenticated", error: "Unauthorized" }, { status: 401 });
    }),
  );
  return calls;
}

async function settle() {
  await flushPromises();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await flushPromises();
}

async function signIn(wrapper: Awaited<ReturnType<typeof mountSuspended>>, email: string, password: string) {
  await wrapper.get("#email").setValue(email);
  await wrapper.get("#password").setValue(password);
  await wrapper.get("form").trigger("submit");
  await settle();
}

describe("login page", () => {
  beforeEach(async () => {
    // One Nuxt app serves every test in the file: forget the user an earlier test signed in, in the
    // module's sign-in state (signing out is a call to the BFF) and in the cache.
    stubBff({ "POST /api/auth/logout": () => new Response(null, { status: 204 }) });
    await useNavreeAuth().signOut();
    vi.unstubAllGlobals();
    navigateTo.mockReset();
    useNuxtApp().$queryClient.clear();
  });
  afterEach(() => vi.unstubAllGlobals());

  it("checks the fields before calling the API", async () => {
    const calls = stubBff({});
    const wrapper = await mountSuspended(LoginPage, { route: "/login" });
    await settle();

    expect(wrapper.get("h1").text()).toBe("Sign in");
    await wrapper.get("form").trigger("submit");

    await vi.waitFor(() => expect(wrapper.text()).toContain("Enter the email address you use for Navree."));
    expect(wrapper.text()).toContain("Enter your password.");
    expect(calls.some((c) => c.route === "POST /api/auth/login")).toBe(false);
  });

  it("signs in through the BFF and goes on to the page in `next`", async () => {
    const calls = stubBff({ "POST /api/auth/login": () => Response.json({ user }) });
    const wrapper = await mountSuspended(LoginPage, { route: "/login?next=/deals" });

    await signIn(wrapper, "admin@navree.test", "password123");

    await vi.waitFor(() => expect(navigateTo).toHaveBeenCalledWith("/deals", { replace: true }));
    expect(calls.find((c) => c.route === "POST /api/auth/login")?.body).toEqual({
      email: "admin@navree.test",
      password: "password123",
    });
    // The module's sign-in state is now signed in, so the module loads the user's abilities.
    await vi.waitFor(() => expect(calls.some((c) => c.route === "GET /api/me/abilities")).toBe(true));
  });

  it("never follows a `next` that leaves the site", async () => {
    stubBff({ "POST /api/auth/login": () => Response.json({ user }) });
    const wrapper = await mountSuspended(LoginPage, { route: "/login?next=//evil.example/steal" });

    await signIn(wrapper, "admin@navree.test", "password123");

    await vi.waitFor(() => expect(navigateTo).toHaveBeenCalledWith("/", { replace: true }));
  });

  it("says plainly when the email and password don't match", async () => {
    stubBff({
      "POST /api/auth/login": () =>
        Response.json({ statusCode: 401, message: "Invalid credentials", error: "Unauthorized" }, { status: 401 }),
    });
    const wrapper = await mountSuspended(LoginPage, { route: "/login" });

    await signIn(wrapper, "admin@navree.test", "wrong");

    await vi.waitFor(() => expect(wrapper.text()).toContain("That email and password don't match an account."));
    expect(navigateTo).not.toHaveBeenCalled();
    expect((wrapper.get("#password").element as HTMLInputElement).value).toBe("");
  });

  it("asks for the authenticator code on a two-factor challenge and sends only the code", async () => {
    const calls = stubBff({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 300 }),
      "POST /api/auth/2fa/verify": () => Response.json({ user }),
    });
    const wrapper = await mountSuspended(LoginPage, { route: "/login?next=/tasks" });

    await signIn(wrapper, "admin@navree.test", "password123");
    await vi.waitFor(() => expect(wrapper.get("h1").text()).toBe("Two-step verification"));
    expect(navigateTo).not.toHaveBeenCalled();

    await wrapper.get("#code").setValue("123 456");
    await wrapper.get("form").trigger("submit");

    await vi.waitFor(() => expect(navigateTo).toHaveBeenCalledWith("/tasks", { replace: true }));
    // The challenge token stays in the BFF's httpOnly cookie; the page sends the code alone.
    expect(calls.find((c) => c.route === "POST /api/auth/2fa/verify")?.body).toEqual({ code: "123456" });
  });
});
