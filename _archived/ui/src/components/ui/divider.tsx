import * as React from "react";
import { cn } from "../../lib/utils";

interface DividerProps extends React.ComponentProps<"div"> {
  readonly type?: "horizontal" | "vertical";
  readonly orientation?: "left" | "center" | "right";
  readonly plain?: boolean;
  readonly children?: React.ReactNode;
}

export const Divider = React.forwardRef<HTMLDivElement, DividerProps>(
  ({ className, type = "horizontal", orientation = "center", plain, children, ...props }, ref) =>
    type === "vertical" ? (
      <div
        ref={ref}
        className={cn("mx-2 inline-block h-[1em] w-px self-stretch bg-[var(--gowl-border)]", className)}
        {...props}
      />
    ) : (
      <div
        ref={ref}
        className={cn(
          "flex w-full items-center whitespace-nowrap text-sm text-[var(--gowl-text-muted)] before:flex-1 before:border-t before:border-[var(--gowl-border)] after:flex-1 after:border-t after:border-[var(--gowl-border)]",
          orientation === "left" && "before:max-w-[5%]",
          orientation === "right" && "after:max-w-[5%]",
          plain && "font-normal",
          className,
        )}
        {...props}
      >
        {children ? <span className="px-3">{children}</span> : null}
      </div>
    ),
);
Divider.displayName = "Divider";
