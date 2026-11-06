import { formatMoney, type MoneyOptions } from "@/lib/format";
import { cn } from "@/lib/utils";

export function Money({
  cents,
  currency = "INR",
  className,
  ...options
}: { cents: number; currency?: string; className?: string } & MoneyOptions) {
  return (
    <span
      className={cn("whitespace-nowrap figures", className)}
      title={options.compact ? formatMoney(cents, currency) : undefined}
    >
      {formatMoney(cents, currency, options)}
    </span>
  );
}
