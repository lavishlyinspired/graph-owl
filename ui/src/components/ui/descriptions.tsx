import * as React from "react";
import { cn } from "../../lib/utils";

interface DescriptionItemData {
  readonly key?: string;
  readonly label: React.ReactNode;
  readonly children: React.ReactNode;
}

interface DescriptionsProps extends Omit<React.ComponentProps<"div">, "title"> {
  readonly title?: React.ReactNode;
  readonly bordered?: boolean;
  readonly column?: number;
  readonly size?: "small" | "default" | "middle";
  readonly layout?: "horizontal" | "vertical";
  readonly items?: readonly DescriptionItemData[];
  readonly children?: React.ReactNode;
}

interface DescriptionsItemProps extends React.ComponentProps<"div"> {
  readonly label: React.ReactNode;
  readonly children: React.ReactNode;
  readonly span?: number;
}

const DescriptionsItem = React.forwardRef<HTMLDivElement, DescriptionsItemProps>(
  ({ className, label, children, span = 1, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        "flex flex-col gap-1 border-[var(--gowl-border-soft)] p-3",
        className,
      )}
      style={{ gridColumn: `span ${span} / span ${span}` }}
      {...props}
    >
      <span className="text-xs text-[var(--gowl-text-muted)]">{label}</span>
      <span className="text-[var(--gowl-text)]">{children}</span>
    </div>
  ),
);
DescriptionsItem.displayName = "DescriptionsItem";

const DescriptionsInternal = React.forwardRef<HTMLDivElement, DescriptionsProps>(
  (
    { className, title, bordered = false, column = 3, size = "default", layout, items, children, ...props },
    ref,
  ) => {
    void layout;
    return (
    <div ref={ref} className={cn("text-sm", size === "small" && "text-xs", className)} {...props}>
      {title ? <div className="mb-3 font-semibold text-[var(--gowl-text)]">{title}</div> : null}
      <div
        className={cn(
          "grid",
          bordered && "rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)]",
        )}
        style={{ gridTemplateColumns: `repeat(${column}, minmax(0, 1fr))` }}
      >
        {items
          ? items.map((item) => (
              <DescriptionsItem key={item.key ?? String(item.label)} label={item.label}>
                {item.children}
              </DescriptionsItem>
            ))
          : children}
      </div>
    </div>
    );
  },
);
DescriptionsInternal.displayName = "Descriptions";

export const Descriptions = Object.assign(DescriptionsInternal, { Item: DescriptionsItem }) as typeof DescriptionsInternal & {
  Item: typeof DescriptionsItem;
};
