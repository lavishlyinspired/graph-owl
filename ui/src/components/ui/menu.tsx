import * as React from "react";
import { cn } from "../../lib/utils";

export interface MenuItem {
  readonly key: string;
  readonly label: React.ReactNode;
  readonly icon?: React.ReactNode;
  readonly disabled?: boolean;
  readonly danger?: boolean;
  readonly children?: MenuItem[];
  readonly onClick?: () => void;
}

interface MenuProps extends Omit<React.ComponentProps<"ul">, "onClick" | "onSelect"> {
  readonly items?: MenuItem[];
  readonly selectedKeys?: string[];
  readonly defaultSelectedKeys?: string[];
  readonly mode?: "vertical" | "horizontal" | "inline";
  readonly theme?: "light" | "dark";
  readonly onClick?: (info: { key: string; keyPath: string[] }) => void;
  readonly onSelect?: (info: { key: string }) => void;
}

function MenuItemRender({
  item,
  depth,
  selectedKeys,
  onClick,
}: {
  item: MenuItem;
  depth: number;
  selectedKeys: string[];
  onClick?: (info: { key: string; keyPath: string[] }) => void;
}) {
  const selected = selectedKeys.includes(item.key);
  const hasChildren = (item.children?.length ?? 0) > 0;
  const [open, setOpen] = React.useState(false);

  return (
    <li>
      <button
        type="button"
        disabled={item.disabled}
        onClick={() => {
          if (hasChildren) setOpen((o) => !o);
          item.onClick?.();
          onClick?.({ key: item.key, keyPath: [item.key] });
        }}
        className={cn(
          "flex w-full items-center gap-2 rounded-[var(--gowl-radius-control)] px-3 py-2 text-sm transition-colors",
          selected
            ? "bg-[var(--gowl-selected)] font-medium text-[var(--gowl-selected-text)]"
            : "text-[var(--gowl-text-muted)] hover:bg-[var(--gowl-fill)] hover:text-[var(--gowl-text)]",
          item.danger && "text-red-600 hover:bg-red-50 hover:text-red-700",
          item.disabled && "pointer-events-none opacity-50",
        )}
        style={{ paddingLeft: `${depth * 12 + 12}px` }}
      >
        {item.icon ? <span className="inline-flex shrink-0">{item.icon}</span> : null}
        <span className="flex-1 truncate text-left">{item.label}</span>
      </button>
      {hasChildren && open ? (
        <ul className="mt-1 flex flex-col gap-0.5">
          {item.children!.map((child) => (
            <MenuItemRender
              key={child.key}
              item={child}
              depth={depth + 1}
              selectedKeys={selectedKeys}
              onClick={onClick}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

export const Menu = React.forwardRef<HTMLUListElement, MenuProps>(
  (
    {
      className,
      items,
      selectedKeys: controlledSelected,
      defaultSelectedKeys,
      mode = "inline",
      theme,
      onClick,
      onSelect,
      ...props
    },
    ref,
  ) => {
    void theme;
    const isControlled = controlledSelected !== undefined;
    const [internal, setInternal] = React.useState<string[]>(defaultSelectedKeys ?? []);
    const selected = isControlled ? controlledSelected : internal;

    return (
      <ul
        ref={ref}
        className={cn(
          "flex",
          mode === "vertical" || mode === "inline" ? "flex-col gap-1" : "flex-row gap-2",
          className,
        )}
        {...props}
      >
        {items?.map((item) => (
          <MenuItemRender
            key={item.key}
            item={item}
            depth={0}
            selectedKeys={selected}
            onClick={(info) => {
              if (!isControlled) setInternal([info.key]);
              onClick?.(info);
              onSelect?.({ key: info.key });
            }}
          />
        ))}
      </ul>
    );
  },
);
Menu.displayName = "Menu";
