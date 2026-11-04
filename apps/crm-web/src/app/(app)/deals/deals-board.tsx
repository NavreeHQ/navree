"use client";

import { Can, useAbility } from "@navree/next/abilities";
import {
  closestCorners,
  DndContext,
  DragOverlay,
  KeyboardSensor,
  MouseSensor,
  TouchSensor,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
  type UniqueIdentifier,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { CalendarIcon, LockIcon, PlusIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { OwnerFilter } from "@/components/filters";
import { Money } from "@/components/money";
import { PageHeader } from "@/components/page-header";
import { StageDot, stageStyle } from "@/components/stage";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { UserAvatar } from "@/components/user-avatar";
import { useCurrentUser } from "@/features/auth/queries";
import { DealFormDialog } from "@/features/deals/deal-form";
import { moveBlockedReason } from "@/features/deals/permissions";
import { LostReasonDialog } from "@/features/deals/lost-reason-dialog";
import { useDealBoard, useMoveDeal } from "@/features/deals/queries";
import { useUrlState } from "@/hooks/use-url-state";
import type { Deal, DealBoard, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import { dueBucket, formatDate, formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";

type Columns = Record<DealStage, string[]>;

const COLUMN_PREFIX = "column:";
const columnId = (stage: DealStage) => `${COLUMN_PREFIX}${stage}`;

function columnsFrom(board: DealBoard | undefined): Columns {
  const columns = Object.fromEntries(DEAL_STAGES.map((s) => [s.stage, [] as string[]])) as unknown as Columns;
  for (const column of board?.stages ?? []) {
    columns[column.stage] = [...column.deals].sort((a, b) => a.position - b.position).map((d) => d.id);
  }
  return columns;
}

function findStage(columns: Columns, id: UniqueIdentifier): DealStage | undefined {
  const key = String(id);
  if (key.startsWith(COLUMN_PREFIX)) return key.slice(COLUMN_PREFIX.length) as DealStage;
  return (Object.keys(columns) as DealStage[]).find((stage) => columns[stage].includes(key));
}

// ---- Card ----------------------------------------------------------------------

function DealCardBody({ deal, lockReason, dragging }: { deal: Deal; lockReason?: string; dragging?: boolean }) {
  const closed = deal.stage === "won" || deal.stage === "lost";
  const due = !closed && deal.expected_close_date ? dueBucket(`${deal.expected_close_date}T23:59:59`) : null;
  return (
    <div
      className={cn(
        "flex flex-col gap-2 rounded-xl border border-line bg-ink-1 p-3 text-left transition-[border-color,background-color] duration-150 group-hover/card:border-line-2 group-hover/card:bg-[color-mix(in_oklab,var(--color-ink-1),var(--color-ink-2))]",
        dragging && "rotate-1 border-line-2 bg-ink-2 shadow-float",
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <Link
          href={`/deals/${deal.id}`}
          className="line-clamp-2 text-[0.8125rem] leading-snug font-medium text-fg hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          {deal.title}
        </Link>
        {lockReason ? (
          <LockIcon className="mt-0.5 size-3 shrink-0 text-fg-3" aria-label={lockReason}>
            <title>{lockReason}</title>
          </LockIcon>
        ) : null}
      </div>
      <span className="truncate text-xs text-fg-3">{deal.company.name}</span>
      {deal.stage === "lost" && deal.lost_reason ? (
        <span className="line-clamp-2 rounded-md border border-danger/20 bg-danger/8 px-2 py-1 text-xs text-fg-2">
          {deal.lost_reason}
        </span>
      ) : null}
      <div className="mt-0.5 flex items-center justify-between gap-2 border-t border-line pt-2">
        <Money
          cents={deal.amount_cents}
          currency={deal.currency}
          className={cn("text-[0.8125rem] font-semibold", deal.stage === "won" ? "text-accent" : "text-fg")}
        />
        <span className="flex items-center gap-2">
          {deal.closed_at || deal.expected_close_date ? (
            <span
              className={cn(
                "inline-flex items-center gap-1 text-xs text-fg-3 figures",
                due === "overdue" && "font-medium text-destructive",
              )}
              title={closed ? "Closed" : "Expected close"}
            >
              <CalendarIcon className="size-3" aria-hidden="true" />
              {formatDate(deal.closed_at ?? deal.expected_close_date)}
            </span>
          ) : null}
          <UserAvatar name={deal.owner.name} id={deal.owner.id} size="xs" showTooltip />
        </span>
      </div>
    </div>
  );
}

function SortableDealCard({ deal, canMove }: { deal: Deal; canMove: boolean }) {
  const router = useRouter();
  const ability = useAbility();
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: deal.id,
    disabled: !canMove,
    data: { deal },
  });
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cn(
        "group/card rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring",
        canMove ? "cursor-grab active:cursor-grabbing" : "cursor-pointer",
        isDragging && "opacity-30",
      )}
      data-testid="deal-card"
      data-deal-id={deal.id}
      data-stage={deal.stage}
      {...attributes}
      {...listeners}
      aria-roledescription={canMove ? "draggable deal" : "deal"}
      // A card you can't move still opens the deal, so it isn't "disabled".
      aria-disabled={undefined}
      aria-label={`${deal.title}, ${deal.company.name}, ${stageLabel(deal.stage)}`}
      onClick={() => router.push(`/deals/${deal.id}`)}
      onKeyDown={(e) => {
        if (e.key === "Enter") router.push(`/deals/${deal.id}`);
        listeners?.onKeyDown?.(e);
      }}
    >
      <DealCardBody
        deal={deal}
        lockReason={
          canMove ? undefined : (moveBlockedReason(ability, deal) ?? `Owned by ${deal.owner.name}; you can't move it`)
        }
      />
    </li>
  );
}

// ---- Column --------------------------------------------------------------------

function StageColumn({
  stage,
  deals,
  canMove,
}: {
  stage: (typeof DEAL_STAGES)[number];
  deals: Deal[];
  canMove: (deal: Deal) => boolean;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: columnId(stage.stage) });
  const total = deals.reduce((sum, d) => sum + d.amount_cents, 0);
  const closed = stage.stage === "won" || stage.stage === "lost";
  return (
    <section
      aria-labelledby={`stage-${stage.stage}`}
      data-stage={stage.stage}
      style={stageStyle(stage.stage)}
      className={cn(
        "relative flex max-h-full w-[17.5rem] shrink-0 flex-col overflow-hidden rounded-xl border bg-ink-1/50 transition-[background-color,border-color] duration-150",
        isOver &&
          "border-[color-mix(in_oklab,var(--stage)_45%,transparent)] bg-[color-mix(in_oklab,var(--stage)_6%,var(--color-ink-1))]",
        closed && stage.stage === "won" && "ml-3",
      )}
    >
      <span aria-hidden="true" className="absolute inset-x-0 top-0 h-0.5 bg-(--stage)" />
      <header className="flex flex-col gap-1 px-3 pt-3 pb-2.5">
        <div className="flex items-center justify-between gap-2">
          <h2 id={`stage-${stage.stage}`} className="flex items-center gap-2 text-sm font-semibold text-fg">
            <StageDot stage={stage.stage} />
            {stage.label}
          </h2>
          {!closed ? <span className="font-mono text-xs leading-5 text-fg-3">{stage.probability}%</span> : null}
        </div>
        <div className="flex items-baseline justify-between gap-2 text-xs text-fg-3">
          <span className="figures" data-testid="column-count">
            {formatNumber(deals.length)} {deals.length === 1 ? "deal" : "deals"}
          </span>
          <Money
            cents={total}
            compact
            className={cn("font-medium", stage.stage === "won" ? "text-accent" : "text-fg-1")}
          />
        </div>
      </header>
      <SortableContext id={columnId(stage.stage)} items={deals.map((d) => d.id)} strategy={verticalListSortingStrategy}>
        <ol ref={setNodeRef} className="flex min-h-24 flex-1 scroll-thin flex-col gap-2 overflow-y-auto px-2 pb-2">
          {deals.map((deal) => (
            <SortableDealCard key={deal.id} deal={deal} canMove={canMove(deal)} />
          ))}
          {deals.length === 0 ? (
            <li className="flex flex-1 items-center justify-center rounded-md border border-dashed border-line-2 px-3 py-8 text-center text-xs text-fg-3">
              {stage.stage === "lost" ? "Drop a deal here to mark it lost" : "No deals in this stage"}
            </li>
          ) : null}
        </ol>
      </SortableContext>
    </section>
  );
}

function BoardSkeleton() {
  return (
    <div className="flex gap-3 overflow-hidden" aria-busy="true">
      {DEAL_STAGES.map((s) => (
        <div key={s.stage} className="flex w-[17.5rem] shrink-0 flex-col gap-2 rounded-xl border bg-ink-1/50 p-2">
          <Skeleton className="h-12 rounded-md" />
          <Skeleton className="h-28 rounded-xl" />
          <Skeleton className="h-28 rounded-xl" />
        </div>
      ))}
    </div>
  );
}

// ---- Board ---------------------------------------------------------------------

type PendingMove = { deal: Deal; stage: DealStage; position: number };

export function DealsBoardView() {
  const url = useUrlState();
  const me = useCurrentUser();
  const ability = useAbility();
  const ownerId = url.get("owner");
  const board = useDealBoard({ owner_id: ownerId });
  const move = useMoveDeal();

  // While dragging (and while the lost-reason dialog is open) the board is driven by local state.
  const [dragColumns, setDragColumns] = useState<Columns | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [origin, setOrigin] = useState<{ stage: DealStage; index: number } | null>(null);
  const [pendingLost, setPendingLost] = useState<PendingMove | null>(null);
  // Bumped by every drag: a move that settles after the next drag has started must not reset that drag.
  const dragGeneration = useRef(0);

  const deals = useMemo(() => {
    const map = new Map<string, Deal>();
    for (const column of board.data?.stages ?? []) for (const d of column.deals) map.set(d.id, d);
    return map;
  }, [board.data]);
  const columns = dragColumns ?? columnsFrom(board.data);
  const canMove = (deal: Deal) => ability.can("move", deal);

  // Mouse drags start after 6px; on touch screens a short press-and-hold starts a drag so columns still scroll.
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: 6 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 220, tolerance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const finish = () => {
    setActiveId(null);
    setOrigin(null);
  };

  /** Sends the move; local columns hold the card in place until the API answers. */
  const commit = (deal: Deal, stage: DealStage, position: number, lostReason?: string) => {
    const generation = dragGeneration.current;
    move.mutate(
      { id: deal.id, stage, position, ...(lostReason ? { lost_reason: lostReason } : {}) },
      {
        onSuccess: () => {
          setPendingLost(null);
          if (stage !== deal.stage) toast.success(`Moved “${deal.title}” to ${stageLabel(stage)}`);
        },
        onSettled: () => {
          if (dragGeneration.current === generation) setDragColumns(null);
        },
      },
    );
  };

  const onDragStart = ({ active }: DragStartEvent) => {
    dragGeneration.current += 1;
    const current = columnsFrom(board.data);
    const stage = findStage(current, active.id);
    if (!stage) return;
    setDragColumns(current);
    setActiveId(String(active.id));
    setOrigin({ stage, index: current[stage].indexOf(String(active.id)) });
  };

  const onDragOver = ({ active, over }: DragOverEvent) => {
    if (!over || !dragColumns) return;
    const from = findStage(dragColumns, active.id);
    const to = findStage(dragColumns, over.id);
    if (!from || !to || from === to) return;
    setDragColumns((prev) => {
      if (!prev) return prev;
      const source = prev[from].filter((id) => id !== String(active.id));
      const target = [...prev[to]];
      const overIndex = target.indexOf(String(over.id));
      target.splice(overIndex >= 0 ? overIndex : target.length, 0, String(active.id));
      return { ...prev, [from]: source, [to]: target };
    });
  };

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    const deal = deals.get(String(active.id));
    if (!over || !dragColumns || !deal || !origin) {
      setDragColumns(null);
      finish();
      return;
    }
    const stage = findStage(dragColumns, active.id)!;
    let ids = dragColumns[stage];
    const overIndex = ids.indexOf(String(over.id));
    const activeIndex = ids.indexOf(String(active.id));
    if (overIndex >= 0 && overIndex !== activeIndex) ids = arrayMove(ids, activeIndex, overIndex);
    const position = ids.indexOf(String(active.id));
    const next = { ...dragColumns, [stage]: ids };
    finish();

    if (stage === origin.stage && position === origin.index) {
      setDragColumns(null);
      return;
    }
    setDragColumns(next);
    if (stage === "lost" && origin.stage !== "lost") {
      setPendingLost({ deal, stage, position }); // hold the card in Lost while we ask why
      return;
    }
    commit(deal, stage, position);
  };

  const activeDeal = activeId ? deals.get(activeId) : undefined;
  const openTotal = board.data?.stages
    .filter((s) => s.stage !== "won" && s.stage !== "lost")
    .reduce((acc, s) => ({ count: acc.count + s.count, amount: acc.amount + s.amount_cents }), { count: 0, amount: 0 });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title="Deals"
        description={
          openTotal ? (
            <>
              {formatNumber(openTotal.count)} open {openTotal.count === 1 ? "deal" : "deals"} worth{" "}
              <Money cents={openTotal.amount} className="font-medium text-fg" />. Drag a card to change its stage.
            </>
          ) : (
            "Your pipeline by stage"
          )
        }
        aside={<OwnerFilter value={ownerId} onChange={(owner) => url.set({ owner })} currentUserId={me.data?.id} />}
        actions={
          <Can I="create" a="Deal">
            <DealFormDialog
              openOnCreateIntent
              trigger={
                <Button>
                  <PlusIcon data-icon="inline-start" />
                  New deal
                </Button>
              }
            />
          </Can>
        }
      />

      {board.isPending ? (
        <BoardSkeleton />
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCorners}
          onDragStart={onDragStart}
          onDragOver={onDragOver}
          onDragEnd={onDragEnd}
          onDragCancel={() => {
            setDragColumns(null);
            finish();
          }}
          accessibility={{
            screenReaderInstructions: {
              draggable:
                "To move a deal, press space or enter to pick it up. Use the arrow keys to move it between stages, then press space or enter to drop it, or escape to cancel.",
            },
          }}
        >
          <div
            className="-mx-4 flex h-[calc(100svh-12.5rem)] min-h-[26rem] scroll-thin gap-3 overflow-x-auto px-4 pb-3 sm:-mx-6 sm:px-6 lg:-mx-8 lg:h-[calc(100svh-13.5rem)] lg:px-8"
            role="region"
            aria-label="Deal board"
          >
            {DEAL_STAGES.map((stage) => (
              <StageColumn
                key={stage.stage}
                stage={stage}
                deals={columns[stage.stage].map((id) => deals.get(id)).filter((d): d is Deal => Boolean(d))}
                canMove={canMove}
              />
            ))}
          </div>
          <DragOverlay dropAnimation={{ duration: 160, easing: "cubic-bezier(0.2, 0, 0, 1)" }}>
            {activeDeal ? (
              <div className="w-[16.5rem] cursor-grabbing" data-testid="deal-drag-overlay">
                <DealCardBody deal={activeDeal} dragging />
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      )}

      <LostReasonDialog
        key={pendingLost?.deal.id ?? "none"}
        open={pendingLost !== null}
        dealTitle={pendingLost?.deal.title ?? ""}
        pending={move.isPending}
        error={move.error}
        onCancel={() => {
          setPendingLost(null);
          setDragColumns(null);
          move.reset();
        }}
        onConfirm={(reason) => pendingLost && commit(pendingLost.deal, pendingLost.stage, pendingLost.position, reason)}
      />
    </div>
  );
}
