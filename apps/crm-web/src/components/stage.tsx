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

export function StageDot({ stage, className }: { stage: DealStage; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block size-2 shrink-0 rounded-[2px]", className)}
      style={{ backgroundColor: stageVar(stage) }}
    />
  );
}

/** Squared tag (like a chart label) tinted with the stage's depth colour. */
export function StageBadge({ stage, className }: { stage: DealStage; className?: string }) {
  return (
    <span
      style={stageStyle(stage)}
      className={cn(
        "inline-flex h-5 items-center gap-1.5 rounded-[4px] stage-wash px-1.5 text-xs font-medium whitespace-nowrap text-foreground",
        className,
      )}
    >
      <StageDot stage={stage} />
      {stageLabel(stage)}
    </span>
  );
}
