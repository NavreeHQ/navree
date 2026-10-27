import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PageActions, ShellSlotsProvider, useActionsSlotRef } from "./shell-slots";
import { breadcrumbFor } from "./top-bar";

describe("breadcrumbFor", () => {
  it("names the sidebar group and the screen", () => {
    expect(breadcrumbFor("/", null)).toEqual({ group: "Sales", trail: [{ label: "Dashboard" }] });
    expect(breadcrumbFor("/companies", null)).toEqual({ group: "Sales", trail: [{ label: "Companies" }] });
    expect(breadcrumbFor("/settings", null)).toEqual({ group: "Workspace", trail: [{ label: "Settings" }] });
  });

  it("links a record back to its list and ends with the record's name, once it has loaded", () => {
    expect(breadcrumbFor("/deals/d1", "Fleet telematics rollout")).toEqual({
      group: "Sales",
      trail: [{ label: "Deals", href: "/deals" }, { label: "Fleet telematics rollout" }],
    });
    expect(breadcrumbFor("/contacts/c1", null).trail).toEqual([{ label: "Contacts", href: "/contacts" }, null]);
  });

  it("names the settings pages", () => {
    expect(breadcrumbFor("/settings/users", null).trail).toEqual([
      { label: "Settings", href: "/settings" },
      { label: "Users and roles" },
    ]);
    expect(breadcrumbFor("/settings/audit", null).trail.at(-1)).toEqual({ label: "Audit log" });
  });

  it("shows nothing for an unknown path", () => {
    expect(breadcrumbFor("/nowhere", null)).toEqual({ trail: [] });
  });
});

function Slot() {
  const ref = useActionsSlotRef();
  return <div data-testid="top-bar-actions" ref={ref} />;
}

describe("PageActions", () => {
  // jsdom has no matchMedia: a desktop-sized window.
  beforeEach(() => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    }));
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("renders in place outside the app shell", () => {
    render(
      <PageActions>
        <button type="button">New deal</button>
      </PageActions>,
    );
    expect(screen.getByRole("button", { name: "New deal" })).toBeTruthy();
  });

  it("moves the actions into the top bar's slot, once", async () => {
    render(
      <ShellSlotsProvider>
        <Slot />
        <main data-testid="page">
          <PageActions>
            <button type="button">New deal</button>
          </PageActions>
        </main>
      </ShellSlotsProvider>,
    );
    await act(async () => {});
    const button = screen.getByRole("button", { name: "New deal" });
    expect(screen.getByTestId("top-bar-actions").contains(button)).toBe(true);
    expect(screen.getByTestId("page").contains(button)).toBe(false);
    expect(screen.getAllByRole("button", { name: "New deal" })).toHaveLength(1);
  });
});
