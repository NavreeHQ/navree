import type { DealBoard, DealStage } from "@/lib/api/types";
import { DEAL_STAGES } from "@/lib/api/types";

/** Card ids per stage, in board order. */
export type Columns = Record<DealStage, string[]>;

export const STAGE_ORDER: readonly DealStage[] = DEAL_STAGES.map((s) => s.stage);

export function columnsFrom(board: DealBoard | undefined): Columns {
  const columns = Object.fromEntries(STAGE_ORDER.map((s) => [s, [] as string[]])) as unknown as Columns;
  for (const column of board?.stages ?? []) {
    columns[column.stage] = [...column.deals].sort((a, b) => a.position - b.position).map((d) => d.id);
  }
  return columns;
}

export function stageOf(columns: Columns, id: string): DealStage | undefined {
  return STAGE_ORDER.find((stage) => columns[stage].includes(id));
}

/** `columns` with card `id` taken out of its stage and put into `stage` at `index` (clamped). */
export function placeCard(columns: Columns, id: string, stage: DealStage, index: number): Columns {
  const next = Object.fromEntries(STAGE_ORDER.map((s) => [s, columns[s].filter((x) => x !== id)])) as unknown as Columns;
  const target = next[stage];
  target.splice(Math.max(0, Math.min(index, target.length)), 0, id);
  return next;
}

/**
 * Where a pointer at `y` drops into a column, given the vertical midpoints of
 * the column's other cards (top to bottom): the index of the first card whose
 * midpoint is below the pointer.
 */
export function dropIndex(midpoints: readonly number[], y: number): number {
  const index = midpoints.findIndex((mid) => y < mid);
  return index === -1 ? midpoints.length : index;
}

/** The stage `step` columns to the left (-1) or right (+1), or the same one at the edges. */
export function neighbourStage(stage: DealStage, step: -1 | 1): DealStage {
  const index = STAGE_ORDER.indexOf(stage);
  return STAGE_ORDER[Math.max(0, Math.min(STAGE_ORDER.length - 1, index + step))]!;
}
