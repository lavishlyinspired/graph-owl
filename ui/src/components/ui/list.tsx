import * as React from "react";
import { cn } from "../../lib/utils";

interface ListProps<T> {
  readonly dataSource: readonly T[];
  readonly renderItem: (item: T, index: number) => React.ReactNode;
  readonly rowKey?: string | ((item: T) => string);
  readonly className?: string;
  readonly style?: React.CSSProperties;
  readonly loading?: boolean;
  readonly locale?: { emptyText?: React.ReactNode };
  readonly children?: React.ReactNode;
  readonly size?: "small" | "default" | "large";
}

interface ListItemProps extends React.ComponentProps<"li"> {
  readonly actions?: React.ReactNode[];
  readonly extra?: React.ReactNode;
}

const Item = React.forwardRef<HTMLLIElement, ListItemProps>(
  ({ className, actions, extra, children, ...props }, ref) => (
    <li
      ref={ref}
      className={cn(
        "flex items-start justify-between border-b border-[var(--gowl-border-soft)] py-3 last:border-b-0",
        className,
      )}
      {...props}
    >
      <div className="flex-1">{children}</div>
      {extra ? <div className="ml-4 shrink-0">{extra}</div> : null}
      {actions ? (
        <div className="ml-4 flex shrink-0 items-center gap-2">
          {actions.map((action, i) => (
            <React.Fragment key={i}>{action}</React.Fragment>
          ))}
        </div>
      ) : null}
    </li>
  ),
);
Item.displayName = "ListItem";

function ListInternal<T>({
  dataSource,
  renderItem,
  rowKey,
  className,
  style,
  loading,
  locale,
  size = "default",
}: ListProps<T>) {
  const keyFor = (item: T) => {
    if (typeof rowKey === "function") return rowKey(item);
    if (typeof rowKey === "string") return String((item as Record<string, unknown>)[rowKey] ?? "");
    return "";
  };

  if (loading) {
    return (
      <div className="flex h-32 items-center justify-center text-[var(--gowl-text-muted)]">
        {locale?.emptyText ?? "Loading…"}
      </div>
    );
  }

  return (
    <ul className={cn("flex flex-col", size === "small" && "text-sm", className)} style={style}>
      {dataSource.map((item, index) => (
        <React.Fragment key={keyFor(item) || index}>
          {renderItem(item, index)}
        </React.Fragment>
      ))}
    </ul>
  );
}

export const List = Object.assign(ListInternal, { Item }) as typeof ListInternal & {
  Item: typeof Item;
};

export type { ListProps };
