"use client";

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useIsMobile } from "@/hooks/use-mobile";

/**
 * Two slots in the top bar that screens fill: the page actions (the primary
 * "New …" button, Edit, Export) and the last breadcrumb (a record's name).
 * Setters and values live in separate contexts, so a screen that registers its
 * title doesn't re-render when the title changes.
 */
type Setters = {
  setActionsSlot: (el: HTMLElement | null) => void;
  setRecordTitle: (title: string | null) => void;
};

const SettersContext = createContext<Setters | null>(null);
const ActionsSlotContext = createContext<HTMLElement | null | undefined>(undefined);
const RecordTitleContext = createContext<string | null>(null);

export function ShellSlotsProvider({ children }: { children: ReactNode }) {
  const [actionsSlot, setActionsSlot] = useState<HTMLElement | null>(null);
  const [recordTitle, setRecordTitle] = useState<string | null>(null);
  const setters = useMemo<Setters>(() => ({ setActionsSlot, setRecordTitle }), []);

  return (
    <SettersContext.Provider value={setters}>
      <ActionsSlotContext.Provider value={actionsSlot}>
        <RecordTitleContext.Provider value={recordTitle}>{children}</RecordTitleContext.Provider>
      </ActionsSlotContext.Provider>
    </SettersContext.Provider>
  );
}

/** The ref callback for the top bar's actions container. */
export function useActionsSlotRef(): ((el: HTMLElement | null) => void) | undefined {
  return useContext(SettersContext)?.setActionsSlot;
}

/** The record name for the last breadcrumb, set by the record screen on show. */
export function useRecordTitle(): string | null {
  return useContext(RecordTitleContext);
}

/** Names the record on screen in the breadcrumb, for as long as the screen is mounted. */
export function useRegisterRecordTitle(title: string | null | undefined) {
  const setters = useContext(SettersContext);
  useEffect(() => {
    if (!setters) return;
    setters.setRecordTitle(title ?? null);
    return () => setters.setRecordTitle(null);
  }, [setters, title]);
}

/**
 * A screen's actions. From `md` up they sit on the right of the top bar (DESIGN.md
 * section 9); on phones, and outside the app shell, they stay in the page header
 * where they are written. Rendered once either way, so dialogs they own mount once.
 */
export function PageActions({ children, className }: { children: ReactNode; className?: string }) {
  const slot = useContext(ActionsSlotContext);
  const isMobile = useIsMobile();
  const inline = <div className={className ?? "flex flex-wrap items-center gap-2"}>{children}</div>;
  // No shell (a test, a page outside the app): render in place.
  if (slot === undefined || isMobile) return inline;
  // The top bar mounts first; until its slot exists there is nowhere to show the actions.
  return slot ? createPortal(<div className="flex items-center gap-2">{children}</div>, slot) : null;
}
