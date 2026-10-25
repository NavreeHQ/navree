import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { initials } from "@/lib/format";
import { cn } from "@/lib/utils";

// Muted sea-glass tints so teammates are told apart without loud colours.
const TINTS = ["#d7e7ec", "#dfe6d7", "#e9e0d2", "#dcdcea", "#e7d9dd", "#d6e4e0"];
const DARK_TINTS = ["#1f3a44", "#2b3a2a", "#3b3326", "#2d2d43", "#3d2a31", "#21403a"];

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
          "bg-(--tint) font-semibold text-foreground/80 dark:bg-(--tint-dark)",
          size === "xs" ? "text-[9px]" : size === "sm" ? "text-[10px]" : "text-xs",
        )}
        style={{ "--tint": TINTS[index], "--tint-dark": DARK_TINTS[index] } as React.CSSProperties}
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
  if (!owner) return <span className="text-muted-foreground">Unassigned</span>;
  return (
    <span className="inline-flex min-w-0 items-center gap-2">
      <UserAvatar name={owner.name} id={owner.id} size="xs" />
      <span className="truncate">{owner.name}</span>
    </span>
  );
}
