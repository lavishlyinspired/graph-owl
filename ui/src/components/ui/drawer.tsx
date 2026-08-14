import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { cn } from "../../lib/utils";
import { CloseOutlined } from "./icons";

interface DrawerProps extends Omit<DialogPrimitive.DialogProps, "onOpenChange"> {
  readonly title?: React.ReactNode;
  readonly width?: number;
  readonly placement?: "left" | "right";
  readonly onClose?: () => void;
  readonly destroyOnHidden?: boolean;
  readonly children: React.ReactNode;
}

export const Drawer = ({
  title,
  width = 420,
  placement = "right",
  onClose,
  children,
  ...props
}: DrawerProps) => (
  <DialogPrimitive.Root onOpenChange={(v) => !v && onClose?.()} {...props}>
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-black/40" />
      <DialogPrimitive.Content
        className={cn(
          "fixed top-0 z-50 h-full max-w-full bg-[var(--gowl-raised)] shadow-[var(--gowl-shadow-large)] outline-none",
          placement === "right" && "right-0 border-l border-[var(--gowl-border)]",
          placement === "left" && "left-0 border-r border-[var(--gowl-border)]",
        )}
        style={{ width }}
      >
        <div className="flex h-full flex-col">
          {title ? (
            <div className="flex items-center justify-between border-b border-[var(--gowl-border)] px-4 py-3">
              <DialogPrimitive.Title className="text-base font-semibold text-[var(--gowl-text)]">
                {title}
              </DialogPrimitive.Title>
              <DialogPrimitive.Close className="rounded-[var(--gowl-radius-small)] p-1 text-[var(--gowl-text-subtle)] hover:bg-[var(--gowl-fill)] hover:text-[var(--gowl-text)] focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)]">
                <CloseOutlined size={14} />
              </DialogPrimitive.Close>
            </div>
          ) : null}
          <div className="flex-1 overflow-auto p-4">{children}</div>
        </div>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  </DialogPrimitive.Root>
);
