import * as React from "react";
import { cn } from "../../lib/utils";

interface StatisticProps extends Omit<React.ComponentProps<"div">, "title" | "prefix"> {
  readonly title: React.ReactNode;
  readonly value: React.ReactNode;
  readonly prefix?: React.ReactNode;
  readonly suffix?: React.ReactNode;
  readonly precision?: number;
  readonly valueStyle?: React.CSSProperties;
}

export const Statistic = React.forwardRef<HTMLDivElement, StatisticProps>(
  (
    { className, title, value, prefix, suffix, precision, valueStyle, ...props },
    ref,
  ) => {
    let formatted = value;
    if (typeof value === "number" && precision !== undefined) {
      formatted = value.toFixed(precision);
    }
    return (
      <div
        ref={ref}
        className={cn("flex flex-col gap-1", className)}
        {...props}
      >
        <span className="text-xs text-[var(--gowl-text-muted)]">{title}</span>
        <span
          className="text-2xl font-semibold text-[var(--gowl-text)]"
          style={valueStyle}
        >
          {prefix ? <span className="mr-1">{prefix}</span> : null}
          {formatted}
          {suffix ? <span className="ml-1">{suffix}</span> : null}
        </span>
      </div>
    );
  },
);
Statistic.displayName = "Statistic";
