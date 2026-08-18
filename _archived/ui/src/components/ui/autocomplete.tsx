import * as React from "react";
import { cn } from "../../lib/utils";
import { Input } from "./input";

interface AutoCompleteOption {
  readonly value: string;
  readonly label?: React.ReactNode;
}

interface AutoCompleteProps {
  readonly options?: AutoCompleteOption[];
  readonly value?: string;
  readonly defaultValue?: string;
  readonly placeholder?: string;
  readonly onChange?: (value: string) => void;
  readonly onSelect?: (value: string) => void;
  readonly onSearch?: (value: string) => void;
  readonly className?: string;
  readonly style?: React.CSSProperties;
  readonly disabled?: boolean;
  readonly children?: React.ReactNode;
}

export const AutoComplete = React.forwardRef<HTMLInputElement, AutoCompleteProps>(
  (
    {
      options = [],
      value,
      defaultValue,
      placeholder,
      onChange,
      onSelect,
      onSearch,
      className,
      style,
      disabled,
      children,
    },
    ref,
  ) => {
    const isControlled = value !== undefined;
    const [internal, setInternal] = React.useState(defaultValue ?? "");
    const [open, setOpen] = React.useState(false);
    const current = isControlled ? value : internal;

    const filtered = options.filter((o) =>
      String(o.label ?? o.value).toLowerCase().includes(current.toLowerCase()),
    );

    return (
      <div className={cn("relative", className)} style={style}>
        {children ? (
          React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
            value: current,
            onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
              const next = e.target.value;
              if (!isControlled) setInternal(next);
              onChange?.(next);
              onSearch?.(next);
              setOpen(true);
            },
            onFocus: () => setOpen(true),
            onBlur: () => setTimeout(() => setOpen(false), 150),
          })
        ) : (
          <Input
            ref={ref}
            value={current}
            placeholder={placeholder}
            disabled={disabled}
            onChange={(e) => {
              const next = e.target.value;
              if (!isControlled) setInternal(next);
              onChange?.(next);
              onSearch?.(next);
              setOpen(true);
            }}
            onFocus={() => setOpen(true)}
            onBlur={() => setTimeout(() => setOpen(false), 150)}
          />
        )}
        {open && filtered.length > 0 ? (
          <ul className="absolute z-50 mt-1 max-h-60 w-full overflow-auto rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] shadow-[var(--gowl-shadow-medium)]">
            {filtered.map((opt) => (
              <li
                key={opt.value}
                className="cursor-pointer px-3 py-2 text-sm text-[var(--gowl-text)] hover:bg-[var(--gowl-fill)]"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  if (!isControlled) setInternal(opt.value);
                  onChange?.(opt.value);
                  onSelect?.(opt.value);
                  setOpen(false);
                }}
              >
                {opt.label ?? opt.value}
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    );
  },
);
AutoComplete.displayName = "AutoComplete";
