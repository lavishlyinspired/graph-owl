import * as React from "react";
import { cn } from "../../lib/utils";

export const Text = React.forwardRef<
  HTMLSpanElement,
  React.ComponentProps<"span"> & {
    readonly type?: "secondary" | "success" | "warning" | "danger";
    readonly strong?: boolean;
    readonly code?: boolean;
    readonly copyable?: boolean;
    readonly ellipsis?: boolean;
    readonly mark?: boolean;
    readonly delete?: boolean;
    readonly italic?: boolean;
  }
>(
  (
    { className, type, strong, code, copyable, ellipsis, mark, delete: deleted, italic, children, ...props },
    ref,
  ) => {
    const Comp = code ? "code" : "span";
    const textRef = React.useRef<HTMLElement>(null);
    const [copied, setCopied] = React.useState(false);
    return (
      <>
        <Comp
          ref={(node: HTMLElement | null) => {
            textRef.current = node;
            if (typeof ref === "function") ref(node as never);
            else if (ref) (ref as React.MutableRefObject<HTMLElement | null>).current = node;
          }}
          className={cn(
            "text-sm text-[var(--gowl-text)]",
            type === "secondary" && "text-[var(--gowl-text-muted)]",
            type === "success" && "text-green-700",
            type === "warning" && "text-amber-700",
            type === "danger" && "text-red-700",
            strong && "font-semibold",
            code &&
              "rounded-[var(--gowl-radius-small)] bg-[var(--gowl-fill)] px-1 py-0.5 font-mono text-xs",
            mark && "rounded-[var(--gowl-radius-small)] bg-amber-100 px-1 py-0.5 text-amber-900",
            ellipsis && "block truncate",
            deleted && "line-through",
            italic && "italic",
            className,
          )}
          {...props}
        >
          {children}
        </Comp>
        {copyable ? (
          <button
            type="button"
            aria-label={copied ? "Copied" : "Copy"}
            className="ml-1 inline-flex text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]"
            onClick={() => {
              void navigator.clipboard.writeText(textRef.current?.textContent ?? "");
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            }}
          >
            {copied ? (
              <svg aria-hidden viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="m4 12 5 5L20 6" />
              </svg>
            ) : (
              <svg aria-hidden viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="12" height="12" rx="2" />
                <path d="M5 15V5a2 2 0 0 1 2-2h10" />
              </svg>
            )}
          </button>
        ) : null}
      </>
    );
  },
);
Text.displayName = "Text";

export const Title = React.forwardRef<
  HTMLHeadingElement,
  React.ComponentProps<"h1"> & {
    readonly level?: 1 | 2 | 3 | 4 | 5;
  }
>(({ className, level = 1, children, ...props }, ref) => {
  const Comp = `h${level}` as const;
  const sizes: Record<number, string> = {
    1: "text-2xl",
    2: "text-xl",
    3: "text-lg",
    4: "text-base",
    5: "text-sm",
  };
  return (
    <Comp
      ref={ref as never}
      className={cn(
        // Real antd's Title carries a bottom margin by default too — most
        // call sites in this codebase already pass `style={{ margin: 0 }}`
        // to zero it out, which only makes sense if a default was assumed;
        // this default now actually exists for the call sites that don't.
        "font-semibold text-[var(--gowl-text)] mb-[0.5em] last:mb-0",
        sizes[level],
        className,
      )}
      {...props}
    >
      {children}
    </Comp>
  );
});
Title.displayName = "Title";

export const Paragraph = React.forwardRef<
  HTMLParagraphElement,
  React.ComponentProps<"p"> & { readonly type?: "secondary"; readonly italic?: boolean }
>(({ className, type, italic, children, ...props }, ref) => (
  <p
    ref={ref}
    className={cn(
      // Real antd's Typography.Paragraph carries a 1em bottom margin by
      // default (part of `.ant-typography`'s base styles) — missing here,
      // a Paragraph followed by any block content (a Space, a list, a
      // card body) ran flush against it with no gap.
      "text-sm leading-relaxed text-[var(--gowl-text)] mb-[1em] last:mb-0",
      type === "secondary" && "text-[var(--gowl-text-muted)]",
      italic && "italic",
      className,
    )}
    {...props}
  >
    {children}
  </p>
));
Paragraph.displayName = "Paragraph";

export const Typography = {
  Text,
  Title,
  Paragraph,
};
