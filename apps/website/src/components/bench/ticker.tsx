import { NumberTicker } from "@/components/magicui/number-ticker";
import { cn } from "@/lib/utils";

const format = (value: number, decimals: number) =>
  new Intl.NumberFormat("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(value);

/**
 * A figure that counts up when it scrolls into view without moving anything
 * around it: an invisible copy of the final value holds the width, and the
 * ticker sits on top of it. The HTML holds the real number.
 */
export function TickerNumber({
  value,
  decimals = 0,
  delay = 0,
  align = "left",
  className,
}: {
  value: number;
  decimals?: number;
  delay?: number;
  align?: "left" | "right";
  className?: string;
}) {
  return (
    <span className={cn("relative inline-block whitespace-nowrap tabular-nums", className)}>
      <span aria-hidden className="invisible">
        {format(value, decimals)}
      </span>
      <NumberTicker
        value={value}
        decimalPlaces={decimals}
        delay={delay}
        className={cn("absolute top-0", align === "left" ? "left-0" : "right-0")}
      />
    </span>
  );
}
