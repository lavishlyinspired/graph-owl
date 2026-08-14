import * as React from "react";
import { cn } from "../../lib/utils";
import { CloseOutlined } from "./icons";

export type BadgeVariant = "default" | "primary" | "success" | "warning" | "error" | "info";

interface BadgeProps extends React.ComponentProps<"span"> {
  readonly variant?: BadgeVariant;
  readonly children?: React.ReactNode;
}

export const badgeVariants: Record<BadgeVariant, string> = {
  default:
    "border border-[var(--gowl-border)] bg-[var(--gowl-fill)] text-[var(--gowl-text)]",
  primary:
    "border border-transparent bg-[var(--gowl-primary)] text-white",
  success: "border border-transparent bg-green-100 text-green-800",
  warning: "border border-transparent bg-amber-100 text-amber-800",
  error: "border border-transparent bg-red-100 text-red-800",
  info: "border border-transparent bg-blue-100 text-blue-800",
};

export const Badge = React.forwardRef<HTMLSpanElement, BadgeProps>(
  ({ className, variant = "default", children, ...props }, ref) => (
    <span
      ref={ref}
      className={cn(
        "inline-flex items-center gap-1 rounded-[var(--gowl-radius-small)] px-2 py-0.5 text-xs font-medium",
        badgeVariants[variant],
        className,
      )}
      {...props}
    >
      {children}
    </span>
  ),
);
Badge.displayName = "Badge";

/** Ant Design `Tag` compatibility wrapper — same call shape, shadcn styling. */
const TagInternal = React.forwardRef<
  HTMLSpanElement,
  Omit<BadgeProps, "variant"> & {
    readonly color?: string;
    readonly closable?: boolean;
    readonly onClose?: () => void;
    readonly icon?: React.ReactNode;
    readonly bordered?: boolean;
  }
>(({ className, color, closable, onClose, icon, bordered = true, children, ...props }, ref) => {
  const style = color ? ({ ["--tag-custom" as string]: color } as React.CSSProperties) : undefined;
  return (
    <span
      ref={ref}
      style={style}
      className={cn(
        "inline-flex items-center gap-1 rounded-[var(--gowl-radius-small)] px-2 py-0.5 text-xs font-medium",
        color
          ? "border-transparent text-white"
          : bordered
            ? "border border-[var(--gowl-border)] bg-[var(--gowl-fill)] text-[var(--gowl-text)]"
            : "border-transparent bg-[var(--gowl-fill)] text-[var(--gowl-text)]",
        className,
      )}
      {...props}
    >
      {icon ? <span className="shrink-0">{icon}</span> : null}
      {children}
      {closable ? (
        <button
          type="button"
          aria-label="Remove"
          onClick={(e) => {
            e.stopPropagation();
            onClose?.();
          }}
          className="ml-0.5 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full opacity-60 hover:opacity-100"
        >
          <CloseOutlined size={10} />
        </button>
      ) : null}
    </span>
  );
});
TagInternal.displayName = "Tag";

interface CheckableTagProps {
  readonly checked: boolean;
  readonly onChange?: (checked: boolean) => void;
  readonly children?: React.ReactNode;
  readonly className?: string;
}

function CheckableTag({ checked, onChange, children, className }: CheckableTagProps) {
  return (
    <button
      type="button"
      aria-pressed={checked}
      onClick={() => onChange?.(!checked)}
      className={cn(
        "inline-flex items-center gap-1 rounded-[var(--gowl-radius-small)] border px-2 py-0.5 text-xs font-medium transition-colors",
        checked
          ? "border-transparent bg-[var(--gowl-primary)] text-white"
          : "border-[var(--gowl-border)] bg-[var(--gowl-fill)] text-[var(--gowl-text)] hover:bg-[var(--gowl-border)]",
        className,
      )}
    >
      {children}
    </button>
  );
}

export const Tag = Object.assign(TagInternal, { CheckableTag }) as typeof TagInternal & {
  CheckableTag: typeof CheckableTag;
};
