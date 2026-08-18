import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--gowl-radius-control)] text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2",
  {
    variants: {
      variant: {
        default: "bg-[var(--gowl-primary)] text-white hover:opacity-90",
        secondary: "bg-[var(--gowl-fill)] text-[var(--gowl-text)] hover:bg-[var(--gowl-border)]",
        outline:
          "border border-[var(--gowl-border)] bg-transparent text-[var(--gowl-text)] hover:bg-[var(--gowl-fill)]",
        ghost: "bg-transparent text-[var(--gowl-text)] hover:bg-[var(--gowl-fill)]",
        destructive: "bg-[var(--gowl-error)] text-white hover:opacity-90",
        link: "text-[var(--gowl-primary)] underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-[var(--gowl-radius-small)] px-3 text-xs",
        lg: "h-10 rounded-[var(--gowl-radius-control)] px-6",
        icon: "size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "type">,
    Omit<VariantProps<typeof buttonVariants>, "size"> {
  readonly asChild?: boolean;
  /** Ant Design compatibility: maps `type` to variant. */
  readonly type?: "button" | "submit" | "reset" | "primary" | "default" | "dashed" | "text" | "link";
  readonly icon?: React.ReactNode;
  readonly htmlType?: "button" | "submit" | "reset";
  readonly danger?: boolean;
  readonly size?: "small" | "middle" | "large" | "default" | "sm" | "lg" | "icon";
  readonly loading?: boolean;
  readonly block?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    { className, variant, size, asChild = false, type, icon, htmlType, danger, loading, block, disabled, children, ...props },
    ref,
  ) => {
    const mappedVariant =
      variant ??
      (danger
        ? "destructive"
        : type === "primary"
          ? "default"
          : type === "text"
            ? "ghost"
            : type === "link"
              ? "link"
              : type === "dashed"
                ? "outline"
                : "secondary");
    const mappedSize =
      size === "small" ? "sm" : size === "large" ? "lg" : size === "middle" ? "default" : size;
    const buttonType =
      type === "primary" || type === "default" || type === "dashed" || type === "text" || type === "link"
        ? htmlType ?? "button"
        : type ?? htmlType ?? "button";
    // `asChild` hands rendering to the caller's single child element via
    // Radix `Slot`, which requires exactly one child — the icon/spinner
    // markup below would add sibling nodes and break that contract. A
    // caller using `asChild` alongside `icon`/`loading` is expected to
    // include them inside its own child, the way `Slot` composition always
    // works.
    if (asChild) {
      return (
        <Slot
          className={cn(buttonVariants({ variant: mappedVariant, size: mappedSize, className }), block && "w-full")}
          ref={ref}
          {...props}
        >
          {children}
        </Slot>
      );
    }
    return (
      <button
        className={cn(buttonVariants({ variant: mappedVariant, size: mappedSize, className }), block && "w-full")}
        ref={ref}
        type={buttonType}
        disabled={disabled || loading}
        {...props}
      >
        {loading ? (
          <svg aria-hidden viewBox="0 0 24 24" className="size-4 animate-spin">
            <circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" strokeWidth="3" strokeDasharray="40" strokeLinecap="round" opacity="0.25" />
            <path d="M12 2a10 10 0 0 1 10 10" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
          </svg>
        ) : icon ? (
          <span className="inline-flex shrink-0">{icon}</span>
        ) : null}
        {children}
      </button>
    );
  },
);
Button.displayName = "Button";

export { buttonVariants };
