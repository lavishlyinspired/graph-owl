import * as React from "react";
import { cn } from "../../lib/utils";
import { CloseOutlined, EyeOutlined } from "./icons";
import { Textarea } from "./textarea";

export interface InputProps extends Omit<React.ComponentProps<"input">, "prefix" | "size"> {
  readonly prefix?: React.ReactNode;
  readonly suffix?: React.ReactNode;
  readonly allowClear?: boolean;
  readonly onPressEnter?: (e: React.KeyboardEvent<HTMLInputElement>) => void;
  readonly size?: "small" | "middle" | "large";
}

const sizeClasses: Record<"small" | "middle" | "large", string> = {
  small: "h-7 text-xs",
  middle: "h-9",
  large: "h-10 text-base",
};

const InputInternal = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, prefix, suffix, allowClear, size = "middle", value, onChange, onPressEnter, onKeyDown, ...props }, ref) => {
    const inputRef = React.useRef<HTMLInputElement>(null);
    const hasValue = String(value ?? "").length > 0;
    return (
      <div
        className={cn(
          "flex w-full items-center gap-2 overflow-hidden rounded-[var(--gowl-radius-control)] border border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-3 text-sm text-[var(--gowl-text)] shadow-sm transition-colors focus-within:ring-2 focus-within:ring-[var(--gowl-primary)] disabled:cursor-not-allowed disabled:opacity-50",
          sizeClasses[size],
          className,
        )}
      >
        {prefix ? <span className="shrink-0 text-[var(--gowl-text-subtle)]">{prefix}</span> : null}
        <input
          type={type}
          ref={(node) => {
            inputRef.current = node;
            if (typeof ref === "function") ref(node);
            else if (ref) (ref as React.MutableRefObject<HTMLInputElement | null>).current = node;
          }}
          value={value}
          onChange={onChange}
          onKeyDown={(e) => {
            onKeyDown?.(e);
            if (e.key === "Enter") onPressEnter?.(e);
          }}
          className="h-full w-full flex-1 bg-transparent py-1 outline-none placeholder:text-[var(--gowl-text-muted)] disabled:bg-transparent"
          {...props}
        />
        {allowClear && hasValue ? (
          <button
            type="button"
            tabIndex={-1}
            aria-label="Clear"
            className="shrink-0 text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]"
            onClick={() => {
              const input = inputRef.current;
              if (!input) return;
              const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value")?.set;
              setter?.call(input, "");
              input.dispatchEvent(new Event("input", { bubbles: true }));
              input.focus();
            }}
          >
            <CloseOutlined size={14} />
          </button>
        ) : null}
        {suffix ? <span className="shrink-0 text-[var(--gowl-text-subtle)]">{suffix}</span> : null}
      </div>
    );
  },
);
InputInternal.displayName = "Input";

const InputPassword = React.forwardRef<HTMLInputElement, Omit<InputProps, "type" | "suffix">>(
  (props, ref) => {
    const [visible, setVisible] = React.useState(false);
    return (
      <InputInternal
        {...props}
        ref={ref}
        type={visible ? "text" : "password"}
        suffix={
          <button
            type="button"
            tabIndex={-1}
            aria-label={visible ? "Hide password" : "Show password"}
            className={cn("shrink-0 text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]", visible && "text-[var(--gowl-text)]")}
            onClick={() => setVisible((v) => !v)}
          >
            <EyeOutlined size={14} />
          </button>
        }
      />
    );
  },
);
InputPassword.displayName = "InputPassword";

export const Input = Object.assign(InputInternal, { TextArea: Textarea, Password: InputPassword }) as typeof InputInternal & {
  TextArea: typeof Textarea;
  Password: typeof InputPassword;
};
