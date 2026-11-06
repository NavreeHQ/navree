import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LoginScreen } from "./login-screen";

const replace = vi.fn();

vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace, push: vi.fn() }),
  useSearchParams: () => new URLSearchParams("next=/deals"),
}));

// The app-wide client calls same-origin relative URLs; give it an absolute base for Node's fetch.
vi.mock("@/lib/api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/client")>();
  return { ...actual, api: actual.createApiClient({ baseUrl: "http://crm.test" }) };
});

const user = { __type: "User", id: "u9", name: "Daljit Kaur", email: "boss@navree.test", roles: ["admin"] };

type Call = { route: string; body: unknown };

/** The BFF as the browser sees it: `POST /api/auth/login`, `POST /api/auth/2fa/verify`, `GET /api/auth/me`. */
function stubBff(routes: Record<string, (body: Record<string, unknown>) => Response>): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: Request) => {
      const route = `${input.method} ${new URL(input.url).pathname}`;
      const text = input.method === "GET" ? "" : await input.text();
      const body = text ? (JSON.parse(text) as Record<string, unknown>) : {};
      calls.push({ route, body });
      const handler = routes[route];
      if (handler) return handler(body);
      return Response.json({ statusCode: 401, message: "Unauthorized", error: "Unauthorized" }, { status: 401 });
    }),
  );
  return calls;
}

function renderLogin() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <LoginScreen />
    </QueryClientProvider>,
  );
  return queryClient;
}

async function signIn(email: string, password: string) {
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: email } });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: password } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
}

describe("Login screen", () => {
  beforeEach(() => replace.mockReset());
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("signs in without a second step when the account has no two-factor authentication", async () => {
    const calls = stubBff({ "POST /api/auth/login": () => Response.json({ user }) });
    const queryClient = renderLogin();

    await signIn("boss@navree.test", "password123");

    await waitFor(() => expect(replace).toHaveBeenCalledWith("/deals"));
    expect(calls.find((c) => c.route === "POST /api/auth/login")?.body).toEqual({
      email: "boss@navree.test",
      password: "password123",
    });
    expect(queryClient.getQueryData(["auth", "user"])).toEqual(user);
  });

  it("asks for the authenticator code when the API answers with a two-factor challenge, then signs in", async () => {
    const calls = stubBff({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 300 }),
      "POST /api/auth/2fa/verify": () => Response.json({ user }),
    });
    const queryClient = renderLogin();

    await signIn("boss@navree.test", "password123");

    expect(await screen.findByRole("heading", { name: "Two-step verification" })).toBeTruthy();
    expect(replace).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: "123 456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));

    await waitFor(() => expect(replace).toHaveBeenCalledWith("/deals"));
    // Only the code goes out; the BFF adds the challenge token from its httpOnly cookie.
    expect(calls.find((c) => c.route === "POST /api/auth/2fa/verify")?.body).toEqual({ code: "123456" });
    expect(queryClient.getQueryData(["auth", "user"])).toEqual(user);
  });

  it("accepts a recovery code instead", async () => {
    const calls = stubBff({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 300 }),
      "POST /api/auth/2fa/verify": () => Response.json({ user }),
    });
    renderLogin();

    await signIn("boss@navree.test", "password123");
    fireEvent.click(await screen.findByRole("button", { name: "Use a recovery code instead" }));
    fireEvent.change(screen.getByLabelText("Recovery code"), { target: { value: "abcd-efgh" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));

    await waitFor(() => expect(replace).toHaveBeenCalledWith("/deals"));
    expect(calls.find((c) => c.route === "POST /api/auth/2fa/verify")?.body).toEqual({ recovery_code: "abcd-efgh" });
  });

  it("shows a wrong code and lets the person try again", async () => {
    let attempts = 0;
    stubBff({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 300 }),
      "POST /api/auth/2fa/verify": () =>
        ++attempts === 1
          ? Response.json(
              {
                statusCode: 401,
                message: "Invalid two-factor code",
                error: "Unauthorized",
                code: "invalid_two_factor_code",
              },
              { status: 401 },
            )
          : Response.json({ user }),
    });
    renderLogin();

    await signIn("boss@navree.test", "password123");
    fireEvent.change(await screen.findByLabelText("Authentication code"), { target: { value: "000000" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(await screen.findByText(/That code isn't right/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: "654321" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/deals"));
  });

  it("goes back to the password step when the challenge has expired", async () => {
    stubBff({
      "POST /api/auth/login": () => Response.json({ two_factor_required: true, expires_in: 300 }),
      "POST /api/auth/2fa/verify": () =>
        Response.json(
          {
            statusCode: 401,
            message: "Your sign-in attempt expired. Sign in again.",
            error: "Unauthorized",
            code: "two_factor_expired",
          },
          { status: 401 },
        ),
    });
    renderLogin();

    await signIn("boss@navree.test", "password123");
    fireEvent.change(await screen.findByLabelText("Authentication code"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeTruthy();
    expect(replace).not.toHaveBeenCalled();
  });
});
