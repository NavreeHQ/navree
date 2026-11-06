import { NavreeError } from "@navree/client";
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyFieldErrors } from "./forms.js";
import { localeAttributes } from "./intl.js";
import { readWorkspaceCookie, useWorkspace, WorkspaceProvider, writeWorkspaceCookie } from "./workspace.js";

afterEach(() => writeWorkspaceCookie(undefined));

function Switcher() {
  const { workspace, setWorkspace } = useWorkspace();
  return <button onClick={() => setWorkspace("gulati")}>{workspace ?? "none"}</button>;
}

describe("workspaces", () => {
  it("starts from the server's workspace and switches through the cookie", () => {
    const onChange = vi.fn();
    render(
      <WorkspaceProvider initial="agra" onChange={onChange}>
        <Switcher />
      </WorkspaceProvider>,
    );
    expect(screen.getByRole("button").textContent).toBe("agra");

    act(() => screen.getByRole("button").click());

    expect(screen.getByRole("button").textContent).toBe("gulati");
    expect(readWorkspaceCookie()).toBe("gulati");
    expect(onChange).toHaveBeenCalledWith("gulati");
  });

  it("reads the cookie when the server didn't say", () => {
    writeWorkspaceCookie("ilaichi");
    render(
      <WorkspaceProvider>
        <Switcher />
      </WorkspaceProvider>,
    );
    expect(screen.getByRole("button").textContent).toBe("ilaichi");
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

  it("renames the API's fields to the form's when given an object, and sends the rest to root.server", () => {
    const setError = vi.fn();
    const error = NavreeError.fromResponse(new Response(null, { status: 400 }), {
      statusCode: 400,
      message: ["company_id is required", "plan is not known", "constructor is odd"],
      error: "Bad Request",
      errors: { company_id: ["company_id is required"], plan: ["plan is not known"], constructor: ["constructor is odd"] },
    });

    expect(applyFieldErrors(error, setError, { company_id: "company", plan: "plan" })).toBe(true);

    expect(setError.mock.calls).toEqual([
      ["company", { type: "server", message: "company_id is required" }],
      ["plan", { type: "server", message: "plan is not known" }],
      // A name that only exists on Object.prototype is not a field of the form.
      ["root.server", { type: "server", message: "constructor is odd" }],
    ]);
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
