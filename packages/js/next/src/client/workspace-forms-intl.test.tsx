import { NavreeError } from "@navree/client";
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyFieldErrors } from "./forms.js";
import { localeAttributes } from "./intl.js";
import { readWorkspaceCookie, useWorkspace, WorkspaceProvider, writeWorkspaceCookie } from "./workspace.js";

afterEach(() => writeWorkspaceCookie(undefined));

function Switcher() {
  const { workspace, setWorkspace } = useWorkspace();
  return <button onClick={() => setWorkspace("globex")}>{workspace ?? "none"}</button>;
}

describe("workspaces", () => {
  it("starts from the server's workspace and switches through the cookie", () => {
    const onChange = vi.fn();
    render(
      <WorkspaceProvider initial="acme" onChange={onChange}>
        <Switcher />
      </WorkspaceProvider>,
    );
    expect(screen.getByRole("button").textContent).toBe("acme");

    act(() => screen.getByRole("button").click());

    expect(screen.getByRole("button").textContent).toBe("globex");
    expect(readWorkspaceCookie()).toBe("globex");
    expect(onChange).toHaveBeenCalledWith("globex");
  });

  it("reads the cookie when the server didn't say", () => {
    writeWorkspaceCookie("initech");
    render(
      <WorkspaceProvider>
        <Switcher />
      </WorkspaceProvider>,
    );
    expect(screen.getByRole("button").textContent).toBe("initech");
    writeWorkspaceCookie(undefined);
    expect(readWorkspaceCookie()).toBeUndefined();
  });
});

describe("applyFieldErrors", () => {
  const validation = NavreeError.fromResponse(new Response(null, { status: 400 }), {
    statusCode: 400,
    message: ["email must be a valid email address", "plan is not known"],
    error: "Bad Request",
    code: "validation_failed",
    errors: { email: ["email must be a valid email address"], plan: ["plan is not known"] },
  });

  it("puts field messages on the form's fields and the rest on root.server", () => {
    const setError = vi.fn();
    expect(applyFieldErrors(validation, setError, ["email", "name"])).toBe(true);
    expect(setError).toHaveBeenCalledWith("email", { type: "server", message: "email must be a valid email address" });
    expect(setError).toHaveBeenCalledWith("root.server", { type: "server", message: "plan is not known" });
  });

  it("ignores errors that aren't validation errors", () => {
    const setError = vi.fn();
    const conflict = NavreeError.fromResponse(new Response(null, { status: 409 }), { statusCode: 409, message: "Taken", error: "Conflict" });
    expect(applyFieldErrors(conflict, setError)).toBe(false);
    expect(applyFieldErrors(new Error("x"), setError)).toBe(false);
    expect(setError).not.toHaveBeenCalled();
  });
});

describe("localeAttributes", () => {
  it("gives <html> its lang and dir", () => {
    expect(localeAttributes("ar-AE")).toEqual({ lang: "ar-AE", dir: "rtl" });
    expect(localeAttributes("hi")).toEqual({ lang: "hi", dir: "ltr" });
  });
});
