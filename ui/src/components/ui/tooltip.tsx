import * as React from "react";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { cn } from "../../lib/utils";

export const TooltipProvider = TooltipPrimitive.Provider;
// The raw, composable Radix API — `<Tooltip><TooltipTrigger asChild>...`.
// Distinct from `AntTooltip` below (the `title`-prop, single-component API
// most call sites use); see `AntTooltip`'s comment for why both exist.
export const Tooltip = TooltipPrimitive.Root;
export const TooltipRoot = TooltipPrimitive.Root;
export const TooltipTrigger = TooltipPrimitive.Trigger;

export const TooltipContent = React.forwardRef<
  React.ElementRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 6, ...props }, ref) => (
  <TooltipPrimitive.Portal>
    <TooltipPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      className={cn(
        "z-50 overflow-hidden rounded-[var(--gowl-radius-small)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-3 py-1.5 text-xs text-[var(--gowl-text)] shadow-[var(--gowl-shadow-medium)]",
        className,
      )}
      {...props}
    />
  </TooltipPrimitive.Portal>
));
TooltipContent.displayName = TooltipPrimitive.Content.displayName;

interface AntTooltipProps {
  readonly title?: React.ReactNode;
  readonly children: React.ReactNode;
  readonly placement?: "top" | "bottom" | "left" | "right";
}

/** Ant Design `Tooltip` compatibility: a single component taking `title`,
 *  unlike the raw Radix primitives above which need an explicit
 *  `TooltipTrigger`/`TooltipContent` composition (used directly by callers
 *  that want that shape instead — see `Popover`/`AntPopover` for the same
 *  split, and why one name cannot serve both call shapes). */
export const AntTooltip = ({ title, children, placement = "top" }: AntTooltipProps) =>
  title ? (
    <TooltipPrimitive.Root delayDuration={200}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipContent side={placement}>{title}</TooltipContent>
    </TooltipPrimitive.Root>
  ) : (
    <>{children}</>
  );
