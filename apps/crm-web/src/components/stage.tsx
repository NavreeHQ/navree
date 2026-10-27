import type { CSSProperties } from "react";
import type { DealStage } from "@/lib/api/types";
import { stageLabel } from "@/lib/api/types";
import { cn } from "@/lib/utils";

/** CSS variable for a stage's depth tint, for inline `--stage` styling. */
export function stageVar(stage: DealStage): string {
  return `var(--stage-${stage})`;
}

export function stageStyle(stage: DealStage): CSSProperties {
  return { "--stage": stageVar(stage) } as CSSProperties;
}

/** A status dot in the stage's color (always shown next to the stage's name). */
export function StageDot({ stage, className }: { stage: DealStage; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block size-2 shrink-0 rounded-full", className)}
      style={{ backgroundColor: stageVar(stage) }}
    />
  );
}

/** A 4px badge tinted with the stage's color, with its dot and its name. */
export function StageBadge({ stage, className }: { stage: DealStage; className?: string }) {
  return (
    <span
      style={stageStyle(stage)}
      className={cn(
        "inline-flex h-5 items-center gap-1.5 rounded-md border border-[color-mix(in_oklab,var(--stage)_30%,transparent)] stage-wash px-1.5 text-xs font-medium whitespace-nowrap text-fg",
        className,
      )}
    >
      <StageDot stage={stage} />
      {stageLabel(stage)}
    </span>
  );
}
