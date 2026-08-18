import * as React from "react";
import { cn } from "../../lib/utils";

interface SegmentedOption<T = string> {
  readonly label: React.ReactNode;
  readonly value: T;
  readonly disabled?: boolean;
}

interface SegmentedProps<T = string> {
  readonly options: Array<T | SegmentedOption<T>>;
  readonly value?: T;
  readonly defaultValue?: T;
  readonly onChange?: (value: T) => void;
  readonly size?: "small" | "middle" | "large";
  readonly disabled?: boolean;
  readonly className?: string;
}

export function Segmented<T extends string>({
  options,
  value,
  defaultValue,
  onChange,
  size = "middle",
  disabled,
  className,
}: SegmentedProps<T>) {
  const isControlled = value !== undefined;
  const [internal, setInternal] = React.useState<T | undefined>(defaultValue);
  const current = isControlled ? value : internal;

  const normalized = options.map((o) =>
    typeof o === "object" && o !== null && "value" in o
      ? (o as SegmentedOption<T>)
      : ({ label: String(o), value: o as T }),
  );

  const sizeClasses = {
    small: "h-7 text-xs",
    middle: "h-9 text-sm",
    large: "h-11 text-base",
  };

  return (
    <div
      className={cn(
        "inline-flex items-center rounded-[var(--gowl-radius-control)] bg-[var(--gowl-fill)] p-1",
        className,
      )}
    >
      {normalized.map((opt) => {
        const active = current === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            disabled={disabled || opt.disabled}
            onClick={() => {
              if (!isControlled) setInternal(opt.value);
              onChange?.(opt.value);
            }}
            className={cn(
              "relative rounded-[var(--gowl-radius-small)] px-3 font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)] disabled:cursor-not-allowed disabled:opacity-50",
              sizeClasses[size],
              active
                ? "bg-[var(--gowl-raised)] text-[var(--gowl-text)] shadow-sm"
                : "text-[var(--gowl-text-muted)] hover:text-[var(--gowl-text)]",
            )}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
