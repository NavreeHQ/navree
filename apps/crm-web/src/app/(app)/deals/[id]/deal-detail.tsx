"use client";

import { Can, useAbility } from "@navree/next/abilities";
import { Building2Icon, CalendarIcon, CheckIcon, PencilIcon, Trash2Icon, UserIcon } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Money } from "@/components/money";
import { DetailRow, Figure, FigureGrid, Panel } from "@/components/page-header";
import { MetaItem, RecordHeader, RecordMissing, RecordSkeleton } from "@/components/record-header";
import { StageBadge, stageStyle } from "@/components/stage";
import { Button } from "@/components/ui/button";
import { OwnerCell } from "@/components/user-avatar";
import { RecordActivity } from "@/features/activities/components";
import { DealFormDialog } from "@/features/deals/deal-form";
import { moveBlockedReason } from "@/features/deals/permissions";
import { LostReasonDialog } from "@/features/deals/lost-reason-dialog";
import { useDeal, useDeleteDeal, useMoveDeal } from "@/features/deals/queries";
import type { Deal, DealStage } from "@/lib/api/types";
import { DEAL_STAGES, stageLabel } from "@/lib/api/types";
import { formatDateLong, formatPercent } from "@/lib/format";
import { cn } from "@/lib/utils";

const BACK = { href: "/deals", label: "Deals" };

/**
 * The deal's path through the pipeline. Stages up to the current one are
 * filled with their depth tint; clicking a stage moves the deal (if allowed).
 */
function StageBar({ deal }: { deal: Deal }) {
  const ability = useAbility();
  const move = useMoveDeal();
  const [askLost, setAskLost] = useState(false);
  const canMove = ability.can("move", deal);
  const currentIndex = DEAL_STAGES.findIndex((s) => s.stage === deal.stage);
  const isLost = deal.stage === "lost";

  const moveTo = (stage: DealStage, lostReason?: string) => {
    move.mutate(
      { id: deal.id, stage, ...(lostReason ? { lost_reason: lostReason } : {}) },
      {
        onSuccess: () => {
          setAskLost(false);
          toast.success(`Moved to ${stageLabel(stage)}`);
        },
      },
    );
  };

  return (
    <div className="flex flex-col gap-2">
      <ol className="flex flex-wrap gap-1" aria-label="Deal stage">
        {DEAL_STAGES.map((s, i) => {
          const current = s.stage === deal.stage;
          const passed = !isLost && s.stage !== "lost" && i < currentIndex;
          const filled = current || passed;
          return (
            <li
              key={s.stage}
              className={cn("min-w-24 flex-1", s.stage === "won" && "ml-2", s.stage === "lost" && "flex-none")}
            >
              <button
                type="button"
                disabled={!canMove || current || move.isPending}
                aria-current={current ? "step" : undefined}
                onClick={() => (s.stage === "lost" ? setAskLost(true) : moveTo(s.stage))}
                style={stageStyle(s.stage)}
                className="group flex w-full flex-col gap-2 rounded-md px-1.5 pt-1.5 pb-2 text-left transition-colors enabled:hover:bg-ink-2 disabled:cursor-default"
                title={canMove && !current ? `Move to ${s.label}` : undefined}
              >
                <span
                  className={cn(
                    "h-1 w-full rounded-xs transition-colors",
                    filled ? "bg-(--stage)" : "bg-ink-3 group-enabled:group-hover:bg-line-2",
                  )}
                  aria-hidden="true"
                />
                <span
                  className={cn(
                    "flex items-center gap-1 text-xs",
                    current ? "font-semibold text-fg" : "text-fg-3 group-enabled:group-hover:text-fg-1",
                  )}
                >
                  {current ? <CheckIcon className="size-3" aria-hidden="true" /> : null}
                  {s.label}
                  {s.stage !== "won" && s.stage !== "lost" ? (
                    <span className="figures opacity-70">{s.probability}%</span>
                  ) : null}
                </span>
              </button>
            </li>
          );
        })}
      </ol>
      {!canMove ? (
        <p className="px-1.5 text-xs text-fg-3">
          {moveBlockedReason(ability, deal) ?? `Only ${deal.owner.name} or a manager can move this deal.`}
        </p>
      ) : null}
      <LostReasonDialog
        open={askLost}
        dealTitle={deal.title}
        pending={move.isPending}
        error={move.error}
        onCancel={() => {
          setAskLost(false);
          move.reset();
        }}
        onConfirm={(reason) => moveTo("lost", reason)}
      />
    </div>
  );
}

function Figures({ deal }: { deal: Deal }) {
  const weighted = Math.round((deal.amount_cents * deal.probability) / 100);
  const closed = deal.stage === "won" || deal.stage === "lost";
  return (
    <FigureGrid columns={4} label="Deal figures">
      <Figure label="Value" highlight={deal.stage === "won"}>
        <Money cents={deal.amount_cents} currency={deal.currency} />
      </Figure>
      <Figure label="Probability">{formatPercent(deal.probability)}</Figure>
      <Figure label="Weighted">
        <Money cents={weighted} currency={deal.currency} />
      </Figure>
      <Figure label={closed ? "Closed" : "Expected close"}>
        {formatDateLong(closed ? deal.closed_at : deal.expected_close_date) || (
          <span className="text-fg-3">Not set</span>
        )}
      </Figure>
    </FigureGrid>
  );
}

export function DealDetail({ id }: { id: string }) {
  const router = useRouter();
  const deal = useDeal(id);
  const remove = useDeleteDeal();

  if (deal.isPending) return <RecordSkeleton />;
  if (deal.isError) return <RecordMissing error={deal.error} noun="deal" back={BACK} />;
  const d = deal.data;

  return (
    <div className="flex flex-col">
      <RecordHeader
        title={d.title}
        eyebrow={<StageBadge stage={d.stage} className="w-fit" />}
        meta={
          <>
            <MetaItem icon={Building2Icon}>
              <Link href={`/companies/${d.company.id}`} className="transition-colors hover:text-fg hover:underline">
                {d.company.name}
              </Link>
            </MetaItem>
            {d.contact ? (
              <MetaItem icon={UserIcon}>
                <Link href={`/contacts/${d.contact.id}`} className="transition-colors hover:text-fg hover:underline">
                  {d.contact.full_name}
                </Link>
              </MetaItem>
            ) : null}
            {d.expected_close_date && !d.closed_at ? (
              <MetaItem icon={CalendarIcon}>Expected to close {formatDateLong(d.expected_close_date)}</MetaItem>
            ) : null}
          </>
        }
        actions={
          <>
            <Can I="update" this={d}>
              <DealFormDialog
                deal={d}
                trigger={
                  <Button variant="outline">
                    <PencilIcon data-icon="inline-start" />
                    Edit
                  </Button>
                }
              />
            </Can>
            <Can I="delete" this={d}>
              <ConfirmDialog
                trigger={
                  <Button variant="ghost" size="icon" aria-label="Delete deal">
                    <Trash2Icon />
                  </Button>
                }
                title={`Delete ${d.title}?`}
                description="The deal is removed from the board and reports. This can't be undone from the app."
                confirmLabel="Delete deal"
                pending={remove.isPending}
                onConfirm={() =>
                  remove.mutate(d.id, {
                    onSuccess: () => {
                      toast.success(`${d.title} deleted`);
                      router.push("/deals");
                    },
                  })
                }
              />
            </Can>
          </>
        }
      />

      <div className="flex flex-col gap-4">
        <Panel bodyClassName="px-3 py-3">
          <StageBar deal={d} />
        </Panel>
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_20rem]">
          <div className="flex min-w-0 flex-col gap-4">
            <Figures deal={d} />
            {d.stage === "lost" && d.lost_reason ? (
              <Panel title="Why it was lost">
                <p className="text-sm">{d.lost_reason}</p>
              </Panel>
            ) : null}
            <Panel title="Activity">
              <RecordActivity subjectType="deal" subjectId={d.id} />
            </Panel>
          </div>
          <Panel title="Details" className="h-fit" bodyClassName="py-2">
            <dl>
              <DetailRow label="Owner">
                <OwnerCell owner={d.owner} />
              </DetailRow>
              <DetailRow label="Company">
                <Link href={`/companies/${d.company.id}`} className="record-link">
                  {d.company.name}
                </Link>
              </DetailRow>
              <DetailRow label="Main contact">
                {d.contact ? (
                  <Link href={`/contacts/${d.contact.id}`} className="record-link">
                    {d.contact.full_name}
                  </Link>
                ) : null}
              </DetailRow>
              <DetailRow label="Currency">{d.currency}</DetailRow>
              <DetailRow label="Created">{formatDateLong(d.created_at)}</DetailRow>
              <DetailRow label="Last updated">{formatDateLong(d.updated_at)}</DetailRow>
            </dl>
          </Panel>
        </div>
      </div>
    </div>
  );
}
