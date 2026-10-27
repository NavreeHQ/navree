import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { initials } from "@/lib/format";
import { cn } from "@/lib/utils";

// Ink with a faint tint each, so teammates are told apart without loud colours.
// Avatars are 4px squares (DESIGN.md section 9), never circles.
const TINTS = ["#17251f", "#1f2619", "#29241c", "#1a2229", "#271d23", "#162625"];

function tintIndex(seed: string): number {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  return hash % TINTS.length;
}

export function UserAvatar({
  name,
  id,
  size = "sm",
  showTooltip = false,
  className,
}: {
  name: string;
  id?: string;
  size?: "xs" | "sm" | "md";
  showTooltip?: boolean;
  className?: string;
}) {
  const index = tintIndex(id ?? name);
  const avatar = (
    <Avatar
      className={cn(size === "xs" ? "size-5" : size === "sm" ? "size-6" : "size-8", className)}
      aria-label={showTooltip ? name : undefined}
    >
      <AvatarFallback
        className={cn(
          "rounded-md border border-line-2 bg-(--tint) font-medium text-fg-1",
          size === "xs" ? "text-[9px]" : size === "sm" ? "text-[10px]" : "text-xs",
        )}
        style={{ "--tint": TINTS[index] } as React.CSSProperties}
      >
        {initials(name)}
      </AvatarFallback>
    </Avatar>
  );
  if (!showTooltip) return avatar;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{avatar}</TooltipTrigger>
      <TooltipContent>{name}</TooltipContent>
    </Tooltip>
  );
}

export function OwnerCell({ owner }: { owner: { id: string; name: string } | null | undefined }) {
  if (!owner) return <span className="text-fg-3">Unassigned</span>;
  return (
    <span className="inline-flex max-w-full min-w-0 items-center gap-2 text-fg-1">
      <UserAvatar name={owner.name} id={owner.id} size="xs" />
      <span className="truncate">{owner.name}</span>
    </span>
  );
}
