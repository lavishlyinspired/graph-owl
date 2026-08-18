import * as React from "react";
import * as SelectPrimitive from "@radix-ui/react-select";
import { cn } from "../../lib/utils";
import { CloseOutlined } from "./icons";

export const SelectRoot = SelectPrimitive.Root;
export const SelectGroup = SelectPrimitive.Group;
export const SelectValue = SelectPrimitive.Value;

interface SelectOption {
  readonly label: React.ReactNode;
  readonly value: string | number | boolean;
  readonly disabled?: boolean;
}

type SelectValue = string | number | boolean | readonly (string | number | boolean)[];

interface AntSelectProps {
  readonly options?: readonly SelectOption[];
  readonly value?: SelectValue;
  readonly defaultValue?: SelectValue;
  readonly onChange?: (value: SelectValue | undefined) => void;
  readonly onValueChange?: (value: SelectValue | undefined) => void;
  readonly placeholder?: string;
  readonly mode?: "multiple" | "tags";
  readonly allowClear?: boolean;
  readonly size?: "small" | "middle" | "large" | "default";
  readonly disabled?: boolean;
  readonly className?: string;
  readonly style?: React.CSSProperties;
  readonly "aria-label"?: string;
  readonly children?: React.ReactNode;
}

function isMultiple(mode: AntSelectProps["mode"], value: SelectValue | undefined): value is readonly (string | number | boolean)[] {
  return mode === "multiple" || mode === "tags" || Array.isArray(value);
}

const RadixPassthroughSelect = React.forwardRef<
  HTMLDivElement,
  AntSelectProps & { readonly children: React.ReactNode }
>(({ value, defaultValue, onChange, onValueChange, disabled, children, ...props }, _ref) => (
  // No `ref` forwarding: `SelectPrimitive.Root` is logic-only and renders no
  // DOM node of its own to attach one to.
  <SelectPrimitive.Root
    value={typeof value === "string" ? value : undefined}
    defaultValue={typeof defaultValue === "string" ? defaultValue : undefined}
    onValueChange={(v) => {
      onChange?.(v);
      onValueChange?.(v);
    }}
    disabled={disabled}
    {...(props as React.ComponentPropsWithoutRef<typeof SelectPrimitive.Root>)}
  >
    {children}
  </SelectPrimitive.Root>
));
RadixPassthroughSelect.displayName = "RadixPassthroughSelect";

const CustomSelect = React.forwardRef<HTMLDivElement, AntSelectProps>(
  (
    {
      className,
      options = [],
      value,
      defaultValue,
      onChange,
      onValueChange,
      placeholder,
      mode,
      allowClear,
      size = "default",
      disabled,
      style,
      ...props
    },
    ref,
  ) => {
    const isMulti = isMultiple(mode, value);
    const [internal, setInternal] = React.useState<SelectValue | undefined>(() => {
      if (defaultValue !== undefined) return defaultValue;
      return isMulti ? [] : undefined;
    });
    const isControlled = value !== undefined;
    const current = isControlled ? value : internal;
    const [open, setOpen] = React.useState(false);
    const containerRef = React.useRef<HTMLDivElement>(null);

    React.useImperativeHandle(ref, () => containerRef.current as HTMLDivElement);

    const selectedSet = React.useMemo(() => {
      if (current === undefined || current === null) return new Set<string | number | boolean>();
      const arr = Array.isArray(current) ? current : [current];
      return new Set(arr);
    }, [current]);

    const findLabel = (v: string | number | boolean | readonly (string | number | boolean)[]) =>
      options.find((o) => !Array.isArray(v) && o.value === v)?.label ?? String(v);

    const toggle = (v: string | number | boolean) => {
      if (isMulti) {
        const arr = Array.isArray(current) ? [...current] : [];
        const idx = arr.indexOf(v);
        const next = idx >= 0 ? [...arr.slice(0, idx), ...arr.slice(idx + 1)] : [...arr, v];
        if (!isControlled) setInternal(next);
        onChange?.(next);
        onValueChange?.(next);
      } else {
        if (!isControlled) setInternal(v);
        onChange?.(v);
        onValueChange?.(v);
        setOpen(false);
      }
    };

    const clear = (e: React.MouseEvent) => {
      e.stopPropagation();
      const next: SelectValue | undefined = isMulti ? [] : undefined;
      if (!isControlled) setInternal(next);
      onChange?.(next);
      onValueChange?.(next);
    };

    React.useEffect(() => {
      if (!open) return undefined;
      const onDoc = (e: MouseEvent) => {
        if (!containerRef.current?.contains(e.target as Node)) setOpen(false);
      };
      document.addEventListener("mousedown", onDoc);
      return () => document.removeEventListener("mousedown", onDoc);
    }, [open]);

    const triggerClass = cn(
      "flex w-full items-center justify-between gap-2 rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-3 text-sm text-[var(--gowl-text)] shadow-sm focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)] disabled:cursor-not-allowed disabled:opacity-50",
      size === "small" && "h-7 px-2 text-xs",
      size === "middle" && "h-8",
      size === "large" && "h-10",
      size !== "small" && size !== "middle" && size !== "large" && "h-9",
    );

    const displayValue = () => {
      if (current === undefined || current === null || (Array.isArray(current) && current.length === 0)) {
        return <span className="text-[var(--gowl-text-muted)]">{placeholder}</span>;
      }
      if (Array.isArray(current)) {
        return (
          <span className="flex flex-1 flex-wrap gap-1 truncate">
            {current.map((v) => (
              <span key={String(v)} className="rounded-[var(--gowl-radius-small)] bg-[var(--gowl-fill)] px-1.5 py-0.5 text-xs">
                {findLabel(v)}
              </span>
            ))}
          </span>
        );
      }
      return <span className="truncate">{findLabel(current)}</span>;
    };

    return (
      <div ref={containerRef} className={cn("relative w-full", className)} style={style} {...props}>
        <button
          type="button"
          aria-label={props["aria-label"]}
          disabled={disabled}
          className={triggerClass}
          onClick={() => setOpen((o) => !o)}
        >
          {displayValue()}
          <span className="inline-flex items-center gap-1">
            {allowClear && selectedSet.size > 0 ? (
              <span
                role="button"
                aria-label="Clear"
                tabIndex={-1}
                className="inline-flex rounded p-0.5 hover:bg-[var(--gowl-fill)]"
                onClick={clear}
              >
                <CloseOutlined size={10} />
              </span>
            ) : null}
            <svg aria-hidden viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" className={cn("transition-transform", open && "rotate-180")}>
              <path d="m6 9 6 6 6-6" />
            </svg>
          </span>
        </button>
        {open ? (
          <div className="absolute z-50 mt-1 max-h-60 w-full overflow-auto rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] shadow-[var(--gowl-shadow-medium)]">
            {options.map((opt) => {
              const selected = selectedSet.has(opt.value);
              return (
                <button
                  key={String(opt.value)}
                  type="button"
                  disabled={opt.disabled}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--gowl-fill)]",
                    selected && "bg-[var(--gowl-fill)] font-medium",
                    opt.disabled && "pointer-events-none opacity-50",
                  )}
                  onClick={() => toggle(opt.value)}
                >
                  {isMulti ? (
                    <span className={cn("h-4 w-4 rounded border", selected ? "border-[var(--gowl-primary)] bg-[var(--gowl-primary)]" : "border-[var(--gowl-border)]")}>
                      {selected ? <svg viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="3" className="h-4 w-4"><path d="m4 12 5 5L20 6" /></svg> : null}
                    </span>
                  ) : null}
                  {opt.label}
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
    );
  },
);
CustomSelect.displayName = "CustomSelect";

export const Select = React.forwardRef<HTMLDivElement, AntSelectProps>((props, ref) =>
  props.children ? (
    <RadixPassthroughSelect {...props} ref={ref}>
      {props.children}
    </RadixPassthroughSelect>
  ) : (
    <CustomSelect {...props} ref={ref} />
  ),
);
Select.displayName = "Select";

export const SelectTrigger = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Trigger>
>(({ className, children, ...props }, ref) => (
  <SelectPrimitive.Trigger
    ref={ref}
    className={cn(
      "flex h-9 w-full items-center justify-between gap-2 rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-3 py-1 text-sm text-[var(--gowl-text)] shadow-sm focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)] disabled:cursor-not-allowed disabled:opacity-50 [&>span]:line-clamp-1",
      className,
    )}
    {...props}
  >
    {children}
    <SelectPrimitive.Icon asChild>
      <svg
        aria-hidden
        viewBox="0 0 24 24"
        width="14"
        height="14"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        className="opacity-60"
      >
        <path d="m6 9 6 6 6-6" />
      </svg>
    </SelectPrimitive.Icon>
  </SelectPrimitive.Trigger>
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;

export const SelectContent = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Content>
>(({ className, children, position = "popper", ...props }, ref) => (
  <SelectPrimitive.Portal>
    <SelectPrimitive.Content
      ref={ref}
      position={position}
      className={cn(
        "z-50 min-w-[8rem] overflow-hidden rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] text-[var(--gowl-text)] shadow-[var(--gowl-shadow-medium)]",
        position === "popper" && "translate-y-1",
        className,
      )}
      {...props}
    >
      <SelectPrimitive.Viewport className="p-1">{children}</SelectPrimitive.Viewport>
    </SelectPrimitive.Content>
  </SelectPrimitive.Portal>
));
SelectContent.displayName = SelectPrimitive.Content.displayName;

export const SelectItem = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Item>
>(({ className, children, ...props }, ref) => (
  <SelectPrimitive.Item
    ref={ref}
    className={cn(
      "relative flex w-full cursor-pointer select-none items-center rounded-[var(--gowl-radius-small)] py-1.5 pl-3 pr-8 text-sm outline-none data-[highlighted]:bg-[var(--gowl-fill)] data-[state=checked]:font-medium",
      className,
    )}
    {...props}
  >
    <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
  </SelectPrimitive.Item>
));
SelectItem.displayName = SelectPrimitive.Item.displayName;
