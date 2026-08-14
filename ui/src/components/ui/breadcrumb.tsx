import * as React from "react";
import { cn } from "../../lib/utils";
import { RightOutlined } from "./icons";

interface BreadcrumbItem {
  readonly title: React.ReactNode;
  readonly href?: string;
  readonly onClick?: () => void;
}

interface BreadcrumbProps extends React.ComponentProps<"nav"> {
  readonly items?: BreadcrumbItem[];
  readonly separator?: React.ReactNode;
  readonly children?: React.ReactNode;
}

const BreadcrumbInternal = React.forwardRef<HTMLElement, BreadcrumbProps>(
  ({ className, items, separator = <RightOutlined size={12} />, children, ...props }, ref) => (
    <nav
      ref={ref}
      aria-label="Breadcrumb"
      className={cn("flex items-center text-sm text-[var(--gowl-text-muted)]", className)}
      {...props}
    >
      {items ? (
        <ol className="flex flex-wrap items-center gap-1">
          {items.map((item, index) => (
            <li key={index} className="flex items-center gap-1">
              {index > 0 ? <span className="mx-1 inline-flex opacity-50">{separator}</span> : null}
              {item.href ? (
                <a
                  href={item.href}
                  onClick={item.onClick}
                  className="hover:text-[var(--gowl-text)] hover:underline"
                >
                  {item.title}
                </a>
              ) : item.onClick ? (
                <button
                  type="button"
                  onClick={item.onClick}
                  className="hover:text-[var(--gowl-text)] hover:underline"
                >
                  {item.title}
                </button>
              ) : (
                <span className="text-[var(--gowl-text)]">{item.title}</span>
              )}
            </li>
          ))}
        </ol>
      ) : (
        children
      )}
    </nav>
  ),
);
BreadcrumbInternal.displayName = "Breadcrumb";

function BreadcrumbItem({
  children,
  href,
  onClick,
}: {
  children: React.ReactNode;
  href?: string;
  onClick?: () => void;
}) {
  return href ? (
    <a href={href} onClick={onClick} className="hover:text-[var(--gowl-text)] hover:underline">
      {children}
    </a>
  ) : onClick ? (
    <button type="button" onClick={onClick} className="hover:text-[var(--gowl-text)] hover:underline">
      {children}
    </button>
  ) : (
    <span className="text-[var(--gowl-text)]">{children}</span>
  );
}

export const Breadcrumb = Object.assign(BreadcrumbInternal, { Item: BreadcrumbItem }) as typeof BreadcrumbInternal & {
  Item: typeof BreadcrumbItem;
};
