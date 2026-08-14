import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "../../lib/utils";
import { Button } from "./button";
import { ExclamationCircleOutlined } from "./icons";

interface PopconfirmProps {
  readonly title: React.ReactNode;
  readonly description?: React.ReactNode;
  readonly onConfirm?: () => void;
  readonly onCancel?: () => void;
  readonly okText?: string;
  readonly cancelText?: string;
  readonly okType?: "primary" | "danger" | "default";
  readonly children: React.ReactNode;
}

export const Popconfirm = ({
  title,
  description,
  onConfirm,
  onCancel,
  okText = "OK",
  cancelText = "Cancel",
  okType = "primary",
  children,
}: PopconfirmProps) => (
  <PopoverPrimitive.Root>
    <PopoverPrimitive.Trigger asChild>{children}</PopoverPrimitive.Trigger>
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        className={cn(
          "z-50 w-72 rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] p-4 shadow-[var(--gowl-shadow-medium)]",
        )}
        sideOffset={4}
        align="center"
      >
        <div className="flex gap-3">
          <span className="mt-0.5 shrink-0 text-amber-500">
            <ExclamationCircleOutlined size={18} />
          </span>
          <div className="flex flex-col gap-2">
            <p className="text-sm font-medium text-[var(--gowl-text)]">{title}</p>
            {description ? (
              <p className="text-xs text-[var(--gowl-text-muted)]">{description}</p>
            ) : null}
            <div className="flex justify-end gap-2 pt-1">
              <PopoverPrimitive.Close asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={onCancel}
                >
                  {cancelText}
                </Button>
              </PopoverPrimitive.Close>
              <PopoverPrimitive.Close asChild>
                <Button
                  type="button"
                  size="sm"
                  variant={okType === "danger" ? "destructive" : "default"}
                  onClick={onConfirm}
                >
                  {okText}
                </Button>
              </PopoverPrimitive.Close>
            </div>
          </div>
        </div>
      </PopoverPrimitive.Content>
    </PopoverPrimitive.Portal>
  </PopoverPrimitive.Root>
);
