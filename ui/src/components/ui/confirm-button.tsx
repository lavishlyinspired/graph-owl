/** shadcn/ui has no direct equivalent of antd's `Popconfirm` — this is the
 *  same "click, see a small inline confirmation, click again to commit"
 *  pattern built on the Popover primitive, so every destructive action in
 *  the console (starting with the Ontology Builder's delete buttons) shares
 *  one implementation rather than each screen inventing its own. */

import { useState } from "react";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";
import { Button, type ButtonProps } from "./button";

interface ConfirmButtonProps extends Omit<ButtonProps, "onClick"> {
  readonly title: string;
  readonly description?: string;
  readonly onConfirm: () => void;
  /** Callers override for i18n; a shared primitive's own default copy is a
   *  plain JS default, never a JSX literal — `local/no-raw-jsx-text` binds
   *  JSX children, not parameter defaults. */
  readonly confirmLabel?: string;
  readonly cancelLabel?: string;
}

export function ConfirmButton({
  title,
  description,
  onConfirm,
  children,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  ...buttonProps
}: ConfirmButtonProps) {
  const [open, setOpen] = useState(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button {...buttonProps} onClick={() => setOpen(true)}>
          {children}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-64">
        <p className="text-sm font-medium text-[var(--gowl-text)]">{title}</p>
        {description && <p className="mt-1 text-xs text-[var(--gowl-text-subtle)]">{description}</p>}
        <div className="mt-3 flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={() => setOpen(false)}>
            {cancelLabel}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => {
              onConfirm();
              setOpen(false);
            }}
          >
            {confirmLabel}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
