import * as React from "react";
import { cn } from "../../lib/utils";

interface TimelineItem {
  readonly children: React.ReactNode;
  readonly dot?: React.ReactNode;
  readonly color?: string;
  readonly label?: React.ReactNode;
}

interface TimelineProps extends React.ComponentProps<"div"> {
  readonly items?: TimelineItem[];
  readonly mode?: "left" | "alternate" | "right";
  readonly children?: React.ReactNode;
}

function TimelineItemRender({
  children,
  dot,
  color = "var(--gowl-primary)",
  label,
}: TimelineItem) {
  return (
    <div className="relative flex gap-4 py-2">
      {label ? (
        <div className="w-20 shrink-0 text-right text-xs text-[var(--gowl-text-muted)]">
          {label}
        </div>
      ) : null}
      <div className="relative flex shrink-0 flex-col items-center">
        <div
          className="z-10 flex h-3 w-3 items-center justify-center rounded-full border-2 border-[var(--gowl-raised)]"
          style={{ backgroundColor: color }}
        >
          {dot}
        </div>
        <div className="absolute top-3 bottom-0 w-px bg-[var(--gowl-border)]" />
      </div>
      <div className="flex-1 pb-4 text-sm text-[var(--gowl-text)]">{children}</div>
    </div>
  );
}

const TimelineInternal = React.forwardRef<HTMLDivElement, TimelineProps>(
  ({ className, items, mode = "left", children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        "relative flex flex-col gap-0",
        mode === "right" && "items-end text-right",
        className,
      )}
      {...props}
    >
      {items
        ? items.map((item, index) => (
            <TimelineItemRender key={index} {...item} />
          ))
        : children}
    </div>
  ),
);
TimelineInternal.displayName = "Timeline";

export const Timeline = Object.assign(TimelineInternal, { Item: TimelineItemRender }) as typeof TimelineInternal & {
  Item: typeof TimelineItemRender;
};
