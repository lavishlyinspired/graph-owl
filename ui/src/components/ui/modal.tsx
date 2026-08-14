import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { cn } from "../../lib/utils";
import { Button } from "./button";
import { CloseOutlined } from "./icons";

interface ModalProps extends DialogPrimitive.DialogProps {
  readonly title?: React.ReactNode;
  readonly open?: boolean;
  readonly onCancel?: () => void;
  readonly onOk?: () => void;
  readonly okText?: string;
  readonly cancelText?: string;
  readonly okType?: "primary" | "danger" | "default";
  readonly okButtonProps?: { readonly disabled?: boolean; readonly danger?: boolean; readonly loading?: boolean };
  readonly cancelButtonProps?: { readonly disabled?: boolean };
  readonly confirmLoading?: boolean;
  readonly children: React.ReactNode;
  readonly footer?: React.ReactNode | null;
  readonly width?: number;
  readonly centered?: boolean;
}

export const Modal = ({
  title,
  open,
  onCancel,
  onOk,
  okText = "OK",
  cancelText = "Cancel",
  okType = "primary",
  okButtonProps,
  cancelButtonProps,
  confirmLoading,
  children,
  footer,
  width = 520,
  centered = false,
  ...props
}: ModalProps) => (
  <DialogPrimitive.Root open={open} onOpenChange={(v) => !v && onCancel?.()} {...props}>
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/40" />
      <DialogPrimitive.Content
        className={cn(
          "fixed z-50 w-full max-w-full rounded-[var(--gowl-radius-modal)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] p-6 shadow-[var(--gowl-shadow-large)] outline-none",
          centered && "left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2",
          !centered && "left-1/2 top-16 -translate-x-1/2",
        )}
        style={{ width }}
      >
        {title ? (
          <DialogPrimitive.Title className="mb-4 text-base font-semibold text-[var(--gowl-text)]">
            {title}
          </DialogPrimitive.Title>
        ) : null}
        <div className="text-sm text-[var(--gowl-text)]">{children}</div>
        {footer === null ? null : footer ? (
          <div className="mt-6 flex justify-end gap-2">{footer}</div>
        ) : (
          <div className="mt-6 flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onCancel} disabled={cancelButtonProps?.disabled}>
              {cancelText}
            </Button>
            <Button
              type="button"
              variant={okButtonProps?.danger || okType === "danger" ? "destructive" : "default"}
              onClick={onOk}
              disabled={confirmLoading || okButtonProps?.disabled}
            >
              {confirmLoading || okButtonProps?.loading ? "Loading…" : okText}
            </Button>
          </div>
        )}
        <DialogPrimitive.Close className="absolute right-4 top-4 rounded-[var(--gowl-radius-small)] p-1 text-[var(--gowl-text-subtle)] hover:bg-[var(--gowl-fill)] hover:text-[var(--gowl-text)] focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)]">
          <CloseOutlined size={14} />
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  </DialogPrimitive.Root>
);
