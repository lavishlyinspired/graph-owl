import * as React from "react";
import { cn } from "../../lib/utils";

interface CardProps extends Omit<React.ComponentProps<"div">, "title"> {
  readonly size?: "small" | "default";
  readonly styles?: { readonly body?: React.CSSProperties };
  readonly title?: React.ReactNode;
  readonly extra?: React.ReactNode;
  readonly hoverable?: boolean;
}

/** True when a caller composes the shadcn-native `CardHeader`/`CardContent`
 *  pattern directly, which already carries its own padding — Card's own
 *  body padding below is for the antd-shaped `title`/`children` path only,
 *  and would double up if applied here too. */
function usesCardSubcomponents(children: React.ReactNode): boolean {
  return React.Children.toArray(children).some(
    (child) =>
      React.isValidElement(child) && (child.type === CardHeader || child.type === CardContent),
  );
}

export const Card = React.forwardRef<HTMLDivElement, CardProps>(
  ({ className, size, styles, style, title, extra, hoverable, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        "flex flex-col rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] shadow-[var(--gowl-shadow-small)]",
        size === "small" && "text-xs",
        hoverable && "transition-shadow hover:shadow-[var(--gowl-shadow-medium)] cursor-pointer",
        className,
      )}
      style={style}
      {...props}
    >
      {(title || extra) ? (
        <div
          className={cn(
            "flex items-center justify-between border-b border-[var(--gowl-border-soft)]",
            size === "small" ? "px-4 py-2.5" : "px-5 py-3.5",
          )}
        >
          {title ? (
            <div className={cn("font-semibold text-[var(--gowl-text)]", size === "small" ? "text-sm" : "text-[15px]")}>
              {title}
            </div>
          ) : (
            <div />
          )}
          {extra ? <div className="flex items-center gap-2">{extra}</div> : null}
        </div>
      ) : null}
      {/* Ant Design's real Card always pads `.ant-card-body` — this compat
       *  layer previously didn't, so every `<Card title="…">raw content</Card>`
       *  call site (the majority pattern in this codebase) rendered its body
       *  flush against the card edges. Skipped when the caller already
       *  composes `CardHeader`/`CardContent` (which self-pad) so it doesn't
       *  double up. */}
      {usesCardSubcomponents(children) ? (
        children
      ) : (
        <div className={cn("flex-1", size === "small" ? "p-4" : "p-5")} style={styles?.body}>
          {children}
        </div>
      )}
    </div>
  ),
);
Card.displayName = "Card";

export const CardHeader = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn("flex flex-col gap-1 p-4", className)} {...props} />
  ),
);
CardHeader.displayName = "CardHeader";

export const CardTitle = React.forwardRef<HTMLHeadingElement, React.ComponentProps<"h3">>(
  ({ className, ...props }, ref) => (
    <h3
      ref={ref}
      className={cn("text-sm font-semibold leading-none text-[var(--gowl-text)]", className)}
      {...props}
    />
  ),
);
CardTitle.displayName = "CardTitle";

export const CardDescription = React.forwardRef<HTMLParagraphElement, React.ComponentProps<"p">>(
  ({ className, ...props }, ref) => (
    <p ref={ref} className={cn("text-xs text-[var(--gowl-text-subtle)]", className)} {...props} />
  ),
);
CardDescription.displayName = "CardDescription";

export const CardContent = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn("p-4 pt-0", className)} {...props} />
  ),
);
CardContent.displayName = "CardContent";

export const CardFooter = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn("flex items-center gap-2 p-4 pt-0", className)} {...props} />
  ),
);
CardFooter.displayName = "CardFooter";
