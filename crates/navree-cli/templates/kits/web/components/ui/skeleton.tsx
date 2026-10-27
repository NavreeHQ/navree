import { cn } from "@/lib/utils"

function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="skeleton"
      className={cn("relative isolate overflow-hidden rounded-md bg-ink-2 before:absolute before:inset-0 before:animate-shimmer before:bg-[linear-gradient(90deg,transparent,rgb(233_239_235/0.045),transparent)]", className)}
      {...props}
    />
  )
}

export { Skeleton }
