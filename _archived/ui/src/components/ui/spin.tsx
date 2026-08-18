import * as React from "react";
import { cn } from "../../lib/utils";

interface SpinProps extends React.ComponentProps<"div"> {
  readonly size?: "small" | "default" | "large";
  readonly spinning?: boolean;
  readonly tip?: React.ReactNode;
  readonly children?: React.ReactNode;
  readonly fullscreen?: boolean;
}

export const Spin = React.forwardRef<HTMLDivElement, SpinProps>(
  (
    {
      className,
      size = "default",
      spinning = true,
      tip,
      children,
      fullscreen,
      ...props
    },
    ref,
  ) => {
    const sizes = {
      small: 16,
      default: 24,
      large: 40,
    };
    const s = sizes[size];
    const indicator = (
      <svg
        width={s}
        height={s}
        viewBox="0 0 24 24"
        className="animate-spin text-[var(--gowl-primary)]"
      >
        <circle
          cx="12"
          cy="12"
          r="10"
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          strokeDasharray="40"
          strokeLinecap="round"
          opacity="0.25"
        />
        <path
          d="M12 2a10 10 0 0 1 10 10"
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          strokeLinecap="round"
        />
      </svg>
    );

    const content = (
      <div
        ref={ref}
        className={cn(
          "inline-flex flex-col items-center justify-center gap-2",
          className,
        )}
        {...props}
      >
        {indicator}
        {tip ? <span className="text-xs text-[var(--gowl-text-muted)]">{tip}</span> : null}
      </div>
    );

    if (!children) {
      return fullscreen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--gowl-page)]/80">
          {content}
        </div>
      ) : (
        content
      );
    }

    return (
      <div className="relative">
        {children}
        {spinning ? (
          <div className="absolute inset-0 z-10 flex items-center justify-center bg-[var(--gowl-page)]/60">
            {content}
          </div>
        ) : null}
      </div>
    );
  },
);
Spin.displayName = "Spin";
