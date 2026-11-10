"use client";

/**
 * The parts of the data table that are not the grid: the search box, a filter's
 * controls, the chips of the filters in force, the bar over the table for the
 * selection and its actions, and the question before a destructive action.
 * They take the list query and hand back the next one; `DataTable` wires them.
 */
import {
  activeFilters,
  clearFilters,
  filterControl,
  groupedCount,
  isFiltered,
  readFilter,
  removeFilterLabel,
  withSearch,
  writeFilter,
  type BulkAction,
  type FilterColumn,
  type ListQuery,
  type SelectionStatus,
  type TableFilterValue,
  type TableLabels,
} from "@navree/client";
import { useEffect, useId, useRef, useState, type InputHTMLAttributes, type ReactNode } from "react";

const inputClass = "h-8 rounded-sm border border-line-2 bg-ink-1 px-2 text-[13px] text-fg-1 placeholder:text-fg-3";
export const buttonClass = "h-8 rounded-sm border border-line-2 px-3 text-[13px] text-fg-1 hover:bg-ink-2 disabled:pointer-events-none disabled:opacity-50";

export interface DebouncedInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "defaultValue" | "onChange"> {
  /** What the list query holds now. */
  value: string;
  /** How long typing waits for a pause, in milliseconds. */
  delay: number;
  /** Called with the text, trimmed, after a pause or on Enter; not called for text it already sent. */
  onCommit: (text: string) => void;
}

/** A text box that tells the list query once typing pauses, and follows the query when it changes without it (back, a chip). */
export function DebouncedInput({ value, delay, onCommit, ...rest }: DebouncedInputProps) {
  const [text, setText] = useState(value);
  const sent = useRef(value.trim());
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (value.trim() !== sent.current) {
      sent.current = value.trim();
      setText(value);
    }
  }, [value]);
  useEffect(() => () => clearTimeout(timer.current), []);

  const send = (next: string) => {
    clearTimeout(timer.current);
    const trimmed = next.trim();
    if (trimmed === sent.current) return;
    sent.current = trimmed;
    onCommit(trimmed);
  };

  return (
    <input
      {...rest}
      value={text}
      onChange={(event) => {
        const next = event.target.value;
        setText(next);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => send(next), delay);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          send(text);
        }
      }}
    />
  );
}

export interface SearchBoxProps {
  query: ListQuery;
  onQueryChange: (next: ListQuery) => void;
  labels: TableLabels;
  delay: number;
}

/** The search box: the list query's `search`, back on the first page. */
export function SearchBox({ query, onQueryChange, labels, delay }: SearchBoxProps) {
  return (
    <div role="search" data-slot="table-search">
      <DebouncedInput
        type="search"
        name="search"
        className={`${inputClass} w-56 max-w-full`}
        placeholder={labels.search}
        aria-label={labels.search}
        autoComplete="off"
        value={query.search ?? ""}
        delay={delay}
        onCommit={(text) => onQueryChange(withSearch(query, text))}
      />
    </div>
  );
}

export interface FilterFieldProps {
  column: FilterColumn;
  query: ListQuery;
  onQueryChange: (next: ListQuery) => void;
  labels: TableLabels;
  delay: number;
  /** In the row under the headings: no visible name, no Empty box, ranges and lists fold away. */
  inRow?: boolean;
}

/** One column's filter: a text box, a range, a list to choose from, ticks, or Any, Yes and No. */
export function FilterField({ column, query, onQueryChange, labels, delay, inRow = false }: FilterFieldProps) {
  const control = filterControl(query, column, labels);
  const [find, setFind] = useState("");
  const set = (value: TableFilterValue | undefined) => onQueryChange(writeFilter(query, column, value));
  const held =
    control.type === "range" ? Number(control.min !== "") + Number(control.max !== "") : control.type === "multi" ? control.values.length : 0;
  let body: ReactNode;
  let folds = false;

  switch (control.type) {
    case "text":
      body = (
        <DebouncedInput
          type="search"
          className={inputClass}
          placeholder={column.label}
          aria-label={`${labels.search}: ${column.label}`}
          autoComplete="off"
          value={control.value}
          delay={delay}
          onCommit={(text) => set(text === "" ? undefined : { type: "text", text })}
        />
      );
      break;
    case "keys":
      body = (
        <DebouncedInput
          type="text"
          className={inputClass}
          aria-label={column.label}
          autoComplete="off"
          value={control.value}
          delay={delay}
          onCommit={(text) => set(text === "" ? undefined : { type: "values", values: [text] })}
        />
      );
      break;
    case "range": {
      folds = true;
      const bound = (name: "min" | "max", word: string) => (
        <label data-slot="filter-field" className="flex items-center gap-2 text-[13px] text-fg-3">
          <span className="w-10">{word}</span>
          <DebouncedInput
            type={control.inputType}
            className={`${inputClass} w-32`}
            aria-label={`${column.label} ${word}`}
            {...(control.step ? { step: control.step } : {})}
            {...(control.inputMode ? { inputMode: control.inputMode } : {})}
            value={control[name]}
            delay={delay}
            onCommit={(text) =>
              set({ type: "range", min: name === "min" ? text : control.min, max: name === "max" ? text : control.max })
            }
          />
        </label>
      );
      body = (
        <div data-slot="filter-range" className="flex flex-col gap-2">
          {bound("min", control.minLabel)}
          {bound("max", control.maxLabel)}
        </div>
      );
      break;
    }
    case "select":
      body = (
        <select
          className={inputClass}
          aria-label={column.label}
          value={control.value}
          onChange={(event) => {
            const value = event.target.value;
            if (value === "") set(undefined);
            else set(column.filter === "boolean" ? { type: "bool", value: value === "true" } : { type: "values", values: [value] });
          }}
        >
          <option value="">{control.prompt}</option>
          {control.choices.map((choice) => (
            <option key={choice.value} value={choice.value}>
              {choice.label}
            </option>
          ))}
        </select>
      );
      break;
    case "multi": {
      folds = true;
      const needle = find.trim().toLowerCase();
      const shown = needle === "" ? control.choices : control.choices.filter((choice) => choice.label.toLowerCase().includes(needle));
      body = (
        <div className="flex flex-col gap-2">
          {control.find ? (
            <input
              type="search"
              className={inputClass}
              placeholder={labels.findOption}
              aria-label={labels.findOption}
              value={find}
              onChange={(event) => setFind(event.target.value)}
            />
          ) : null}
          <div data-slot="filter-options" className="flex max-h-56 flex-col gap-1 overflow-y-auto">
            {shown.map((choice) => (
              <label key={choice.value} data-slot="filter-option" className="flex items-center gap-2 text-[13px] text-fg-1">
                <input
                  type="checkbox"
                  checked={control.values.includes(choice.value)}
                  onChange={(event) =>
                    set({
                      type: "values",
                      values: event.target.checked ? [...control.values, choice.value] : control.values.filter((value) => value !== choice.value),
                    })
                  }
                />
                <bdi>{choice.label}</bdi>
                {choice.count !== undefined ? (
                  <>
                    {" "}
                    <span data-slot="filter-count" className="figures ms-auto text-fg-3">
                      {groupedCount(choice.count, labels.grouping)}
                    </span>
                  </>
                ) : null}
              </label>
            ))}
          </div>
        </div>
      );
      break;
    }
  }

  const empty =
    column.nullable && !inRow ? (
      <label data-slot="filter-option" className="flex items-center gap-2 text-[13px] text-fg-1">
        <input
          type="checkbox"
          checked={readFilter(query, column)?.type === "empty"}
          onChange={(event) => set(event.target.checked ? { type: "empty" } : undefined)}
        />
        <span>{labels.emptyValue}</span>
      </label>
    ) : null;

  if (inRow) {
    return folds ? (
      <details data-slot="filter-popover" data-field={column.field}>
        <summary className="cursor-pointer text-[13px] text-fg-3">
          {column.label}
          {held ? ` (${held})` : ""}
        </summary>
        <div className="pt-2">{body}</div>
      </details>
    ) : (
      <div data-slot="filter-field" data-field={column.field}>
        {body}
      </div>
    );
  }
  return (
    <div role="group" aria-label={column.label} data-slot="filter-field" data-field={column.field} className="flex min-w-0 flex-col gap-1.5">
      <span aria-hidden="true" className="text-[12px] font-medium text-fg-3">
        {column.label}
      </span>
      {body}
      {empty}
    </div>
  );
}

export interface ChipsProps {
  query: ListQuery;
  columns: readonly FilterColumn[];
  onQueryChange: (next: ListQuery) => void;
  labels: TableLabels;
}

/** The search and filters in force, each with a way off, and Clear filters. */
export function Chips({ query, columns, onQueryChange, labels }: ChipsProps) {
  if (!isFiltered(query)) return null;
  const chip = (key: string, text: string, off: () => void) => (
    <button
      key={key}
      type="button"
      data-slot="filter-chip"
      aria-label={removeFilterLabel(labels, text)}
      onClick={off}
      className="inline-flex h-7 items-center gap-1 rounded-sm border border-line-2 px-2 text-[13px] text-fg-1 hover:bg-ink-2"
    >
      <bdi>{text}</bdi>
      <span aria-hidden="true">×</span>
    </button>
  );
  const search = (query.search ?? "").trim();
  return (
    <div data-slot="table-chips" className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
      {search === "" ? null : chip("search", `${labels.search}: ${search}`, () => onQueryChange(withSearch(query, "")))}
      {activeFilters(query, columns, labels).map((active) => {
        const column = columns.find((item) => item.field === active.field);
        return chip(active.field, active.text, () => (column ? onQueryChange(writeFilter(query, column, undefined)) : undefined));
      })}
      <button type="button" data-slot="chips-clear" className="text-[13px] text-fg-2 underline hover:text-fg" onClick={() => onQueryChange(clearFilters(query))}>
        {labels.clearFilters}
      </button>
    </div>
  );
}

/** A bulk action with its icon, as the table draws it. */
export interface BulkActionView extends BulkAction {
  icon?: ReactNode;
}

export interface BulkBarProps {
  status: SelectionStatus;
  /** Say the selection of all results ended. */
  ended: boolean;
  actions: readonly BulkActionView[];
  busy: boolean;
  labels: TableLabels;
  onAction: (action: BulkActionView) => void;
  onSelectAll: () => void;
  onClear: () => void;
}

/**
 * The bar over the table: how many rows are selected, the offer to select every result, Clear selection and
 * the actions. Its status line is always in the page, so a screen reader hears the count change.
 */
export function BulkBar({ status, ended, actions, busy, labels, onAction, onSelectAll, onClear }: BulkBarProps) {
  const visible = status.count > 0 || ended;
  const clear = useRef<HTMLButtonElement>(null);
  return (
    <div
      data-slot="table-bulk"
      data-has-selection={visible ? "" : undefined}
      {...(visible ? { role: "group", "aria-label": labels.actions } : {})}
      className={visible ? "flex flex-wrap items-center gap-3 border-b border-line bg-ink-2 px-3 py-2 text-[13px] text-fg-1" : "sr-only"}
    >
      <span data-slot="bulk-count" role="status" aria-live="polite" className="figures">
        {visible ? (ended ? labels.selectionEnded : status.text) : ""}
      </span>
      {visible && status.offerAll ? (
        <button
          type="button"
          data-slot="bulk-link"
          className="underline hover:text-fg"
          onClick={() => {
            onSelectAll();
            clear.current?.focus();
          }}
        >
          {status.allText}
        </button>
      ) : null}
      {visible ? (
        <button ref={clear} type="button" data-slot="bulk-link" className="underline hover:text-fg" onClick={onClear}>
          {labels.clearSelection}
        </button>
      ) : null}
      {visible && actions.length > 0 ? (
        <div data-slot="bulk-actions" className="ms-auto flex flex-wrap gap-2">
          {actions.map((action) => (
            <button
              key={action.id}
              type="button"
              name="action"
              value={action.id}
              disabled={action.denied !== undefined || busy}
              title={action.denied}
              className={`${buttonClass} inline-flex items-center gap-1.5 ${action.destructive ? "text-danger" : ""}`}
              onClick={() => onAction(action)}
            >
              {action.icon}
              {action.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export interface ConfirmDialogProps {
  text: string;
  labels: TableLabels;
  onYes: () => void;
  onCancel: () => void;
}

/** The question before an action that asked for one. A modal dialog: Escape and Cancel close it, focus returns to the button. */
export function ConfirmDialog({ text, labels, onYes, onCancel }: ConfirmDialogProps) {
  const id = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const answered = useRef(false);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    try {
      dialog.showModal();
    } catch {
      // No modal dialogs here, or not in the page: it is shown open instead.
    }
    if (!dialog.open) dialog.setAttribute("open", "");
  }, []);
  // Closing it first puts focus back on the button that asked.
  const answer = (yes: boolean) => {
    answered.current = true;
    const dialog = ref.current;
    if (dialog && typeof dialog.close === "function") dialog.close();
    (yes ? onYes : onCancel)();
  };
  return (
    <dialog
      ref={ref}
      role="alertdialog"
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-text`}
      onClose={() => {
        if (!answered.current) onCancel();
      }}
      className="m-auto w-[min(26rem,calc(100vw-2rem))] rounded-sm border border-line-2 bg-ink-1 p-4 text-fg-1 backdrop:bg-ink-0/70"
    >
      <h2 id={`${id}-title`} className="text-[15px] font-medium text-fg">
        {labels.confirmTitle}
      </h2>
      <p id={`${id}-text`} className="mt-2 text-[13px] text-fg-2">
        {text}
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" className={buttonClass} onClick={() => answer(false)}>
          {labels.cancel}
        </button>
        <button type="button" className={`${buttonClass} border-danger text-danger`} onClick={() => answer(true)}>
          {labels.confirm}
        </button>
      </div>
    </dialog>
  );
}
