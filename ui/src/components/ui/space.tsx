import * as React from "react";
import { cn } from "../../lib/utils";

interface SpaceProps extends React.ComponentProps<"div"> {
  readonly direction?: "horizontal" | "vertical";
  readonly size?: "small" | "middle" | "large" | number | [number, number];
  readonly align?: "start" | "end" | "center" | "baseline";
  readonly wrap?: boolean;
  readonly split?: React.ReactNode;
  readonly children: React.ReactNode;
}

const sizeMap: Record<string, string> = {
  small: "gap-1",
  middle: "gap-2",
  large: "gap-4",
};

const alignMap: Record<string, string> = {
  start: "items-start",
  end: "items-end",
  center: "items-center",
  baseline: "items-baseline",
};

const SpaceInternal = React.forwardRef<HTMLDivElement, SpaceProps>(
  (
    {
      className,
      direction = "horizontal",
      size = "small",
      align,
      wrap = false,
      split,
      children,
      style,
      ...props
    },
    ref,
  ) => {
    const gapStyle: React.CSSProperties | undefined =
      typeof size === "number"
        ? { gap: size }
        : Array.isArray(size)
          ? { rowGap: size[0], columnGap: size[1] }
          : undefined;
    const items = React.Children.toArray(children);
    // Ant Design centers by default only for *horizontal* Space; vertical
    // Space leaves `align-items` unset, so children stretch to the
    // container's full width (the CSS default for a column flex is
    // `align-items: normal`, which computes to `stretch`). Forcing
    // `items-center` unconditionally here — the bug this replaces — made
    // every card/table/row centered at its own intrinsic width instead of
    // filling the page, which is why every screen built on a vertical
    // `Space` wrapper looked narrow and misaligned.
    const resolvedAlign = align ?? (direction === "horizontal" ? "center" : undefined);
    return (
      <div
        ref={ref}
        className={cn(
          "flex",
          direction === "vertical" ? "flex-col" : "flex-row",
          typeof size === "string" && sizeMap[size],
          resolvedAlign && alignMap[resolvedAlign],
          wrap && "flex-wrap",
          className,
        )}
        style={{ ...gapStyle, ...style }}
        {...props}
      >
        {items.map((child, index) => (
          <React.Fragment key={index}>
            {child}
            {split && index < items.length - 1 ? (
              <span className="inline-flex items-center">{split}</span>
            ) : null}
          </React.Fragment>
        ))}
      </div>
    );
  },
);
SpaceInternal.displayName = "Space";

/** Ant Design `Space.Compact`: joins children edge-to-edge with no gap, as a
 *  single visually-merged control group (e.g. an input + button row). */
const SpaceCompact = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        "flex [&>*]:rounded-none [&>*:first-child]:rounded-l-[var(--gowl-radius-control)] [&>*:last-child]:rounded-r-[var(--gowl-radius-control)] [&>*:not(:first-child)]:-ml-px",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  ),
);
SpaceCompact.displayName = "SpaceCompact";

export const Space = Object.assign(SpaceInternal, { Compact: SpaceCompact }) as typeof SpaceInternal & {
  Compact: typeof SpaceCompact;
};

const justifyMap: Record<string, string> = {
  start: "justify-start",
  "flex-start": "justify-start",
  end: "justify-end",
  "flex-end": "justify-end",
  center: "justify-center",
  // Tailwind's real utility names — "justify-space-between" isn't one, so
  // this silently no-opped and every `justify="space-between"` Flex (e.g.
  // the Overview "What is in the catalog" rows) rendered with no gap
  // between label and value instead of pushing them to opposite edges.
  "space-between": "justify-between",
  "space-around": "justify-around",
};

const alignMapFlex: Record<string, string> = {
  start: "items-start",
  "flex-start": "items-start",
  end: "items-end",
  "flex-end": "items-end",
  center: "items-center",
  stretch: "items-stretch",
  baseline: "items-baseline",
};

export const Flex = React.forwardRef<
  HTMLDivElement,
  React.ComponentProps<"div"> & {
    readonly vertical?: boolean;
    readonly justify?: "start" | "end" | "center" | "space-between" | "space-around" | "flex-start" | "flex-end";
    readonly align?: "start" | "end" | "center" | "stretch" | "flex-start" | "flex-end" | "baseline";
    readonly gap?: "small" | "middle" | "large" | number;
    readonly wrap?: boolean | "wrap" | "nowrap" | "wrap-reverse";
  }
>(
  (
    {
      className,
      vertical = false,
      justify = "start",
      align = "stretch",
      gap,
      wrap = false,
      children,
      style,
      ...props
    },
    ref,
  ) => {
    const shouldWrap = wrap === true || (typeof wrap === "string" && wrap !== "nowrap");
    const gapStyle =
      typeof gap === "number" ? ({ gap } as React.CSSProperties) : undefined;
    return (
      <div
        ref={ref}
        className={cn(
          "flex",
          vertical ? "flex-col" : "flex-row",
          justifyMap[justify],
          alignMapFlex[align],
          gap === "small" && "gap-1",
          gap === "middle" && "gap-2",
          gap === "large" && "gap-4",
          shouldWrap && "flex-wrap",
          className,
        )}
        style={{ ...gapStyle, ...style }}
        {...props}
      >
        {children}
      </div>
    );
  },
);
Flex.displayName = "Flex";

export const Row = React.forwardRef<
  HTMLDivElement,
  React.ComponentProps<"div"> & {
    readonly gutter?: number | [number, number];
    readonly justify?: "start" | "end" | "center" | "space-between";
  }
>(({ className, gutter, justify = "start", style, children, ...props }, ref) => {
  const horizontalGutter = Array.isArray(gutter) ? gutter[0] : gutter;
  // Same fix as Flex's justifyMap: Tailwind's real class is `justify-between`,
  // not `justify-space-between` — the naive template literal below produced
  // a class that doesn't exist and silently no-opped.
  const justifyClass = justify === "space-between" ? "justify-between" : `justify-${justify}`;
  return (
    <div
      ref={ref}
      className={cn("flex flex-wrap", justifyClass, className)}
      style={{
        ...style,
        marginLeft: horizontalGutter ? -horizontalGutter / 2 : undefined,
        marginRight: horizontalGutter ? -horizontalGutter / 2 : undefined,
      }}
      {...props}
    >
      {children}
    </div>
  );
});
Row.displayName = "Row";

export const Col = React.forwardRef<
  HTMLDivElement,
  React.ComponentProps<"div"> & {
    span?: number;
    xs?: number;
    sm?: number;
    md?: number;
    lg?: number;
    xl?: number;
    flex?: string;
    gutter?: number;
  }
>(({ className, span = 24, xs, sm, md, lg, xl, flex, gutter, style, children, ...props }, ref) => {
  const responsiveSpan = xl ?? lg ?? md ?? sm ?? xs ?? span;
  const pct = responsiveSpan ? `${(responsiveSpan / 24) * 100}%` : undefined;
  return (
    <div
      ref={ref}
      className={cn("box-border", className)}
      style={{ ...style, width: flex ? undefined : pct, flex: flex ? flex : undefined, paddingLeft: gutter ? gutter / 2 : undefined, paddingRight: gutter ? gutter / 2 : undefined }}
      {...props}
    >
      {children}
    </div>
  );
});
Col.displayName = "Col";
