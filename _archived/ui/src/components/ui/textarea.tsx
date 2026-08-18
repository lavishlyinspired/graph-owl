import * as React from "react";
import { cn } from "../../lib/utils";

interface TextareaProps extends React.ComponentProps<"textarea"> {
  /** Ant Design compatibility: an approximate row-based min/max height,
   *  rather than true content-tracking auto-resize. */
  readonly autoSize?: boolean | { minRows?: number; maxRows?: number };
}

const ROW_HEIGHT_PX = 20;

export const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, autoSize, style, ...props }, ref) => {
    const sizeStyle: React.CSSProperties | undefined =
      autoSize && typeof autoSize === "object"
        ? {
            minHeight: autoSize.minRows ? autoSize.minRows * ROW_HEIGHT_PX : undefined,
            maxHeight: autoSize.maxRows ? autoSize.maxRows * ROW_HEIGHT_PX : undefined,
          }
        : undefined;
    return (
      <textarea
        ref={ref}
        className={cn(
          "flex min-h-16 w-full rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-3 py-2 text-sm text-[var(--gowl-text)] shadow-sm transition-colors placeholder:text-[var(--gowl-text-muted)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--gowl-primary)] disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        style={{ ...sizeStyle, ...style }}
        {...props}
      />
    );
  },
);
Textarea.displayName = "Textarea";
