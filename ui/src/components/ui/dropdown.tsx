import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "../../lib/utils";

export interface DropdownMenuItem {
  readonly key: string;
  readonly label: React.ReactNode;
  readonly type?: "group";
  readonly disabled?: boolean;
  readonly children?: readonly DropdownMenuItem[];
}

interface DropdownMenuConfig {
  readonly items: readonly DropdownMenuItem[];
  readonly selectedKeys?: readonly string[];
  readonly onClick?: (info: { key: string }) => void;
}

interface DropdownProps {
  readonly children: React.ReactElement;
  readonly menu: DropdownMenuConfig;
  readonly trigger?: readonly ("click" | "hover")[];
  readonly disabled?: boolean;
}

function DropdownItemRow({
  item,
  selectedKeys,
  onClick,
}: {
  item: DropdownMenuItem;
  selectedKeys: readonly string[];
  onClick?: (info: { key: string }) => void;
}) {
  const selected = selectedKeys.includes(item.key);
  return (
    <button
      type="button"
      role="menuitem"
      disabled={item.disabled}
      onClick={() => onClick?.({ key: item.key })}
      className={cn(
        "flex w-full items-center gap-2 rounded-[var(--gowl-radius-small)] px-2 py-1.5 text-left text-sm transition-colors",
        selected
          ? "bg-[var(--gowl-selected)] font-medium text-[var(--gowl-selected-text)]"
          : "text-[var(--gowl-text)] hover:bg-[var(--gowl-fill)]",
        item.disabled && "pointer-events-none opacity-50",
      )}
    >
      {item.label}
    </button>
  );
}

/** Ant Design `Dropdown` compatibility: a `menu={{ items, selectedKeys, onClick }}`
 *  config rather than the raw Radix trigger/content composition, and support
 *  for antd's flat "group" item shape (a non-interactive label followed by
 *  its `children`, not a collapsible submenu). */
export function Dropdown({ children, menu, disabled }: DropdownProps) {
  const [open, setOpen] = React.useState(false);
  const selectedKeys = menu.selectedKeys ?? [];

  return (
    <PopoverPrimitive.Root open={disabled ? false : open} onOpenChange={setOpen}>
      <PopoverPrimitive.Trigger asChild disabled={disabled}>
        {children}
      </PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="start"
          sideOffset={6}
          className="z-50 min-w-[10rem] rounded-[var(--gowl-radius-card)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] p-1 text-[var(--gowl-text)] shadow-[var(--gowl-shadow-medium)] outline-none"
        >
          <div role="menu" className="flex flex-col gap-0.5">
            {menu.items.map((item) =>
              item.type === "group" ? (
                <div key={item.key} className="flex flex-col gap-0.5 py-1">
                  <div className="px-2 py-1 text-xs font-medium text-[var(--gowl-text-subtle)]">
                    {item.label}
                  </div>
                  {item.children?.map((child) => (
                    <DropdownItemRow
                      key={child.key}
                      item={child}
                      selectedKeys={selectedKeys}
                      onClick={(info) => {
                        menu.onClick?.(info);
                        setOpen(false);
                      }}
                    />
                  ))}
                </div>
              ) : (
                <DropdownItemRow
                  key={item.key}
                  item={item}
                  selectedKeys={selectedKeys}
                  onClick={(info) => {
                    menu.onClick?.(info);
                    setOpen(false);
                  }}
                />
              ),
            )}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
