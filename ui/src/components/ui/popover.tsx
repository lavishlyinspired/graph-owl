import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "../../lib/utils";

export const Popover = PopoverPrimitive.Root;
export const PopoverTrigger = PopoverPrimitive.Trigger;
export const PopoverAnchor = PopoverPrimitive.Anchor;

export const PopoverContent = React.forwardRef<
  React.ElementRef<typeof PopoverPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className, align = "start", sideOffset = 6, ...props }, ref) => (
  <PopoverPrimitive.Portal>
    <PopoverPrimitive.Content
      ref={ref}
      align={align}
      sideOffset={sideOffset}
      className={cn(
        "z-50 rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] p-4 text-[var(--gowl-text)] shadow-[var(--gowl-shadow-medium)] outline-none",
        className,
      )}
      {...props}
    />
  </PopoverPrimitive.Portal>
));
PopoverContent.displayName = PopoverPrimitive.Content.displayName;

interface AntPopoverProps {
  readonly children: React.ReactNode;
  readonly content?: React.ReactNode;
  readonly title?: React.ReactNode;
  readonly open?: boolean;
  readonly onOpenChange?: (open: boolean) => void;
  readonly trigger?: "click" | "hover" | readonly ("click" | "hover")[];
  readonly placement?:
    | "top"
    | "topLeft"
    | "topRight"
    | "bottom"
    | "bottomLeft"
    | "bottomRight"
    | "left"
    | "right";
}

const placementMap: Record<
  NonNullable<AntPopoverProps["placement"]>,
  { side: "top" | "bottom" | "left" | "right"; align: "start" | "center" | "end" }
> = {
  top: { side: "top", align: "center" },
  topLeft: { side: "top", align: "start" },
  topRight: { side: "top", align: "end" },
  bottom: { side: "bottom", align: "center" },
  bottomLeft: { side: "bottom", align: "start" },
  bottomRight: { side: "bottom", align: "end" },
  left: { side: "left", align: "center" },
  right: { side: "right", align: "center" },
};

/** Ant Design `Popover` compatibility: a single component taking
 *  `content`/`trigger`/`placement`, unlike the raw Radix primitives above
 *  which need an explicit `PopoverTrigger`/`PopoverContent` composition. */
export function AntPopover({
  children,
  content,
  title,
  open,
  onOpenChange,
  placement = "bottom",
}: AntPopoverProps) {
  const { side, align } = placementMap[placement];
  return (
    <PopoverPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <PopoverPrimitive.Trigger asChild>{children}</PopoverPrimitive.Trigger>
      <PopoverContent side={side} align={align}>
        {title ? <div className="mb-2 font-medium text-[var(--gowl-text)]">{title}</div> : null}
        {content}
      </PopoverContent>
    </PopoverPrimitive.Root>
  );
}
