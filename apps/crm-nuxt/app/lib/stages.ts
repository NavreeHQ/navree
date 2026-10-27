import type { DealStage } from "@/lib/api/types";

/** The CSS color of a stage on the pipeline ramp (`--color-stage-*` in main.css). */
export function stageColor(stage: DealStage): string {
  return `var(--color-stage-${stage})`;
}

/** Inline style that sets `--stage` for a stage, for tints like `bg-(--stage)/10`. */
export function stageStyle(stage: DealStage): Record<string, string> {
  return { "--stage": stageColor(stage) };
}

export function isClosed(stage: DealStage): boolean {
  return stage === "won" || stage === "lost";
}
