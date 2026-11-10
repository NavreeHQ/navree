/**
 * The parts of the data table that are not the grid: the search box, a filter's
 * controls, the chips of the filters in force, the bar over the table for the
 * selection and its actions, and the question before a destructive action.
 * They take the list query and hand back the next one; `NavreeDataTable` wires
 * them. The markup, names and words are the React table's (`@navree/next`).
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
import { defineComponent, h, nextTick, onBeforeUnmount, onMounted, ref, watch, type PropType, type Ref, type VNode, type VNodeChild } from "vue";

const inputClass = "h-8 rounded-sm border border-line-2 bg-ink-1 px-2 text-[13px] text-fg-1 placeholder:text-fg-3";
export const buttonClass = "h-8 rounded-sm border border-line-2 px-3 text-[13px] text-fg-1 hover:bg-ink-2 disabled:pointer-events-none disabled:opacity-50";

/** Puts a checkbox back to what the table says after a change it did not take (a list query held by the page). */
function keep(target: EventTarget | null, read: () => boolean): void {
  const box = target as HTMLInputElement;
  void nextTick(() => {
    box.checked = read();
  });
}

/** A text box that tells the list query once typing pauses, and follows the query when it changes without it (back, a chip). */
export const DebouncedInput = defineComponent({
  name: "NavreeDebouncedInput",
  inheritAttrs: false,
  props: {
    /** What the list query holds now. */
    value: { type: String, required: true },
    /** How long typing waits for a pause, in milliseconds. */
    delay: { type: Number, required: true },
  },
  /** The text, trimmed, after a pause or on Enter; not emitted for text already sent. */
  emits: { commit: (text: string) => typeof text === "string" },
  setup(props, { attrs, emit }) {
    const text = ref(props.value);
    let sent = props.value.trim();
    let timer: ReturnType<typeof setTimeout> | undefined;
    watch(
      () => props.value,
      (value) => {
        if (value.trim() !== sent) {
          sent = value.trim();
          text.value = value;
        }
      },
    );
    onBeforeUnmount(() => clearTimeout(timer));
    const send = (next: string) => {
      clearTimeout(timer);
      const trimmed = next.trim();
      if (trimmed === sent) return;
      sent = trimmed;
      emit("commit", trimmed);
    };
    return () =>
      h("input", {
        ...attrs,
        value: text.value,
        onInput: (event: Event) => {
          const next = (event.target as HTMLInputElement).value;
          text.value = next;
          clearTimeout(timer);
          timer = setTimeout(() => send(next), props.delay);
        },
        onKeydown: (event: KeyboardEvent) => {
          if (event.key === "Enter") {
            event.preventDefault();
            send(text.value);
          }
        },
      });
  },
});

/** The search box: the list query's `search`, back on the first page. */
export function renderSearchBox(query: ListQuery, labels: TableLabels, delay: number, onQuery: (next: ListQuery) => void): VNode {
  return h("div", { role: "search", "data-slot": "table-search" }, [
    h(DebouncedInput, {
      type: "search",
      name: "search",
      class: `${inputClass} w-56 max-w-full`,
      placeholder: labels.search,
      "aria-label": labels.search,
      autocomplete: "off",
      value: query.search ?? "",
      delay,
      onCommit: (text: string) => onQuery(withSearch(query, text)),
    }),
  ]);
}

/** One column's filter: a text box, a range, a list to choose from, ticks, or Any, Yes and No. */
export const FilterField = defineComponent({
  name: "NavreeFilterField",
  props: {
    column: { type: Object as PropType<FilterColumn>, required: true },
    query: { type: Object as PropType<ListQuery>, required: true },
    labels: { type: Object as PropType<TableLabels>, required: true },
    delay: { type: Number, required: true },
    /** In the row under the headings: no visible name, no Empty box, ranges and lists fold away. */
    inRow: { type: Boolean, default: false },
  },
  emits: { "update:query": (query: ListQuery) => typeof query === "object" },
  setup(props, { emit }) {
    const find = ref("");
    const set = (value: TableFilterValue | undefined) => emit("update:query", writeFilter(props.query, props.column, value));
    return () => {
      const { column, query, labels, delay, inRow } = props;
      const control = filterControl(query, column, labels);
      const held = control.type === "range" ? Number(control.min !== "") + Number(control.max !== "") : control.type === "multi" ? control.values.length : 0;
      let body: VNodeChild;
      let folds = false;

      switch (control.type) {
        case "text":
          body = h(DebouncedInput, {
            type: "search",
            class: inputClass,
            placeholder: column.label,
            "aria-label": `${labels.search}: ${column.label}`,
            autocomplete: "off",
            value: control.value,
            delay,
            onCommit: (text: string) => set(text === "" ? undefined : { type: "text", text }),
          });
          break;
        case "keys":
          body = h(DebouncedInput, {
            type: "text",
            class: inputClass,
            "aria-label": column.label,
            autocomplete: "off",
            value: control.value,
            delay,
            onCommit: (text: string) => set(text === "" ? undefined : { type: "values", values: [text] }),
          });
          break;
        case "range": {
          folds = true;
          const bound = (name: "min" | "max", word: string) =>
            h("label", { "data-slot": "filter-field", class: "flex items-center gap-2 text-[13px] text-fg-3" }, [
              h("span", { class: "w-10" }, word),
              h(DebouncedInput, {
                type: control.inputType,
                class: `${inputClass} w-32`,
                "aria-label": `${column.label} ${word}`,
                ...(control.step ? { step: control.step } : {}),
                ...(control.inputMode ? { inputmode: control.inputMode } : {}),
                value: control[name],
                delay,
                onCommit: (text: string) =>
                  set({ type: "range", min: name === "min" ? text : control.min, max: name === "max" ? text : control.max }),
              }),
            ]);
          body = h("div", { "data-slot": "filter-range", class: "flex flex-col gap-2" }, [
            bound("min", control.minLabel),
            bound("max", control.maxLabel),
          ]);
          break;
        }
        case "select": {
          body = h(
            "select",
            {
              class: inputClass,
              "aria-label": column.label,
              value: control.value,
              onChange: (event: Event) => {
                const select = event.target as HTMLSelectElement;
                const value = select.value;
                if (value === "") set(undefined);
                else set(column.filter === "boolean" ? { type: "bool", value: value === "true" } : { type: "values", values: [value] });
                // A page that holds the list query may not take the change: the box shows what the query says.
                void nextTick(() => {
                  const latest = filterControl(props.query, props.column, props.labels);
                  select.value = latest.type === "select" ? latest.value : "";
                });
              },
            },
            [
              h("option", { value: "" }, control.prompt),
              ...control.choices.map((choice) => h("option", { key: choice.value, value: choice.value }, choice.label)),
            ],
          );
          break;
        }
        case "multi": {
          folds = true;
          const needle = find.value.trim().toLowerCase();
          const shown = needle === "" ? control.choices : control.choices.filter((choice) => choice.label.toLowerCase().includes(needle));
          body = h("div", { class: "flex flex-col gap-2" }, [
            control.find
              ? h("input", {
                  type: "search",
                  class: inputClass,
                  placeholder: labels.findOption,
                  "aria-label": labels.findOption,
                  value: find.value,
                  onInput: (event: Event) => {
                    find.value = (event.target as HTMLInputElement).value;
                  },
                })
              : null,
            h(
              "div",
              { "data-slot": "filter-options", class: "flex max-h-56 flex-col gap-1 overflow-y-auto" },
              shown.map((choice) => {
                const on = control.values.includes(choice.value);
                return h("label", { key: choice.value, "data-slot": "filter-option", class: "flex items-center gap-2 text-[13px] text-fg-1" }, [
                  h("input", {
                    type: "checkbox",
                    checked: on,
                    onChange: (event: Event) => {
                      const box = event.target as HTMLInputElement;
                      set({
                        type: "values",
                        values: box.checked ? [...control.values, choice.value] : control.values.filter((value) => value !== choice.value),
                      });
                      keep(box, () => {
                        const latest = filterControl(props.query, props.column, props.labels);
                        return latest.type === "multi" && latest.values.includes(choice.value);
                      });
                    },
                  }),
                  h("bdi", choice.label),
                  choice.count !== undefined
                    ? [" ", h("span", { "data-slot": "filter-count", class: "figures ms-auto text-fg-3" }, groupedCount(choice.count, labels.grouping))]
                    : null,
                ]);
              }),
            ),
          ]);
          break;
        }
      }

      const empty =
        column.nullable && !inRow
          ? h("label", { "data-slot": "filter-option", class: "flex items-center gap-2 text-[13px] text-fg-1" }, [
              h("input", {
                type: "checkbox",
                checked: readFilter(query, column)?.type === "empty",
                onChange: (event: Event) => {
                  const box = event.target as HTMLInputElement;
                  set(box.checked ? { type: "empty" } : undefined);
                  keep(box, () => readFilter(props.query, column)?.type === "empty");
                },
              }),
              h("span", labels.emptyValue),
            ])
          : null;

      if (inRow) {
        return folds
          ? h("details", { "data-slot": "filter-popover", "data-field": column.field }, [
              h("summary", { class: "cursor-pointer text-[13px] text-fg-3" }, `${column.label}${held ? ` (${held})` : ""}`),
              h("div", { class: "pt-2" }, [body]),
            ])
          : h("div", { "data-slot": "filter-field", "data-field": column.field }, [body]);
      }
      return h(
        "div",
        { role: "group", "aria-label": column.label, "data-slot": "filter-field", "data-field": column.field, class: "flex min-w-0 flex-col gap-1.5" },
        [h("span", { "aria-hidden": "true", class: "text-[12px] font-medium text-fg-3" }, column.label), body, empty],
      );
    };
  },
});

/** The search and filters in force, each with a way off, and Clear filters. */
export function renderChips(query: ListQuery, columns: readonly FilterColumn[], labels: TableLabels, onQuery: (next: ListQuery) => void): VNode | null {
  if (!isFiltered(query)) return null;
  const chip = (key: string, text: string, off: () => void) =>
    h(
      "button",
      {
        key,
        type: "button",
        "data-slot": "filter-chip",
        "aria-label": removeFilterLabel(labels, text),
        class: "inline-flex h-7 items-center gap-1 rounded-sm border border-line-2 px-2 text-[13px] text-fg-1 hover:bg-ink-2",
        onClick: off,
      },
      [h("bdi", text), h("span", { "aria-hidden": "true" }, "×")],
    );
  const search = (query.search ?? "").trim();
  return h("div", { "data-slot": "table-chips", class: "flex flex-wrap items-center gap-2 border-b border-line px-3 py-2" }, [
    search === "" ? null : chip("search", `${labels.search}: ${search}`, () => onQuery(withSearch(query, ""))),
    ...activeFilters(query, columns, labels).map((active) => {
      const column = columns.find((item) => item.field === active.field);
      return chip(active.field, active.text, () => {
        if (column) onQuery(writeFilter(query, column, undefined));
      });
    }),
    h(
      "button",
      {
        type: "button",
        "data-slot": "chips-clear",
        class: "text-[13px] text-fg-2 underline hover:text-fg",
        onClick: () => onQuery(clearFilters(query)),
      },
      labels.clearFilters,
    ),
  ]);
}

/** A bulk action with an icon, as the table draws it. */
export interface BulkActionView extends BulkAction {
  icon?: VNodeChild | (() => VNodeChild);
}

export interface BulkBarProps {
  status: SelectionStatus;
  /** Say the selection of all results ended. */
  ended: boolean;
  actions: readonly BulkActionView[];
  busy: boolean;
  labels: TableLabels;
  /** Takes the focus when "Select all" is chosen, so the offer's place is not lost. */
  clear: Ref<HTMLButtonElement | null>;
  onAction: (action: BulkActionView) => void;
  onSelectAll: () => void;
  onClear: () => void;
}

/**
 * The bar over the table: how many rows are selected, the offer to select every result, Clear selection and
 * the actions. Its status line is always in the page, so a screen reader hears the count change.
 */
export function renderBulkBar({ status, ended, actions, busy, labels, clear, onAction, onSelectAll, onClear }: BulkBarProps): VNode {
  const visible = status.count > 0 || ended;
  return h(
    "div",
    {
      "data-slot": "table-bulk",
      "data-has-selection": visible ? "" : undefined,
      ...(visible ? { role: "group", "aria-label": labels.actions } : {}),
      class: visible ? "flex flex-wrap items-center gap-3 border-b border-line bg-ink-2 px-3 py-2 text-[13px] text-fg-1" : "sr-only",
    },
    [
      h("span", { "data-slot": "bulk-count", role: "status", "aria-live": "polite", class: "figures" }, visible ? (ended ? labels.selectionEnded : status.text) : ""),
      visible && status.offerAll
        ? h(
            "button",
            {
              type: "button",
              "data-slot": "bulk-link",
              class: "underline hover:text-fg",
              onClick: () => {
                onSelectAll();
                clear.value?.focus();
              },
            },
            status.allText,
          )
        : null,
      visible
        ? h("button", { ref: clear, type: "button", "data-slot": "bulk-link", class: "underline hover:text-fg", onClick: onClear }, labels.clearSelection)
        : null,
      visible && actions.length > 0
        ? h(
            "div",
            { "data-slot": "bulk-actions", class: "ms-auto flex flex-wrap gap-2" },
            actions.map((action) =>
              h(
                "button",
                {
                  key: action.id,
                  type: "button",
                  name: "action",
                  value: action.id,
                  disabled: action.denied !== undefined || busy,
                  title: action.denied,
                  class: `${buttonClass} inline-flex items-center gap-1.5 ${action.destructive ? "text-danger" : ""}`,
                  onClick: () => onAction(action),
                },
                [typeof action.icon === "function" ? action.icon() : action.icon, action.label],
              ),
            ),
          )
        : null,
    ],
  );
}

let dialogs = 0;

/** The question before an action that asked for one. A modal dialog: Escape and Cancel close it, focus returns to the button. */
export const ConfirmDialog = defineComponent({
  name: "NavreeConfirmDialog",
  props: {
    text: { type: String, required: true },
    labels: { type: Object as PropType<TableLabels>, required: true },
  },
  emits: { yes: () => true, cancel: () => true },
  setup(props, { emit }) {
    const dialog = ref<HTMLDialogElement | null>(null);
    const id = `navree-confirm-${++dialogs}`;
    let answered = false;
    onMounted(() => {
      const el = dialog.value;
      if (!el || el.open) return;
      try {
        el.showModal();
      } catch {
        // Not in the page (a table drawn out of the document): it is shown open instead.
      }
      if (!el.open) el.setAttribute("open", "");
    });
    // Closing it first puts focus back on the button that asked.
    const answer = (yes: boolean) => {
      answered = true;
      const el = dialog.value;
      if (el && typeof el.close === "function") el.close();
      if (yes) emit("yes");
      else emit("cancel");
    };
    return () =>
      h(
        "dialog",
        {
          ref: dialog,
          role: "alertdialog",
          "aria-labelledby": `${id}-title`,
          "aria-describedby": `${id}-text`,
          onClose: () => {
            if (!answered) emit("cancel");
          },
          class: "m-auto w-[min(26rem,calc(100vw-2rem))] rounded-sm border border-line-2 bg-ink-1 p-4 text-fg-1 backdrop:bg-ink-0/70",
        },
        [
          h("h2", { id: `${id}-title`, class: "text-[15px] font-medium text-fg" }, props.labels.confirmTitle),
          h("p", { id: `${id}-text`, class: "mt-2 text-[13px] text-fg-2" }, props.text),
          h("div", { class: "mt-4 flex justify-end gap-2" }, [
            h("button", { type: "button", class: buttonClass, onClick: () => answer(false) }, props.labels.cancel),
            h("button", { type: "button", class: `${buttonClass} border-danger text-danger`, onClick: () => answer(true) }, props.labels.confirm),
          ]),
        ],
      );
  },
});
