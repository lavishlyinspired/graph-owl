import * as React from "react";
import { cn } from "../../lib/utils";

/** Recurses into `Fragment` children (e.g. a conditional's `<>...</>` branch)
 *  the way Ant Design's own `Layout` does — `React.Children.toArray` treats a
 *  Fragment as one opaque element and does not look inside it, so a `Sider`
 *  nested one conditional-branch deep would otherwise go undetected. */
function containsSider(children: React.ReactNode): boolean {
  return React.Children.toArray(children).some((child) => {
    if (!React.isValidElement(child)) return false;
    if (child.type === Sider) return true;
    if (child.type === React.Fragment) {
      return containsSider((child.props as { children?: React.ReactNode }).children);
    }
    return false;
  });
}

const LayoutInternal = React.forwardRef<
  HTMLDivElement,
  React.ComponentProps<"div"> & { readonly hasSider?: boolean }
>(({ className, hasSider, children, ...props }, ref) => {
  // Ant Design's real `Layout` auto-detects a `Sider` child and switches to
  // a row so the sider sits beside content rather than above it — callers
  // do not pass `hasSider` explicitly. `hasSider` stays as an override for
  // the rare case detection still misses (a `Sider` produced by a custom
  // component rather than appearing directly or inside a `Fragment`).
  const detectedHasSider = containsSider(children);
  return (
    // No `min-h-screen`: Ant Design's real `Layout` sizes from its content
    // or an explicit `style.height`/parent flex, never a forced viewport
    // minimum. Hardcoding one here broke every *nested* `Layout` (e.g. the
    // agent chat composer, meant to fill its parent's remaining height) —
    // `min-height: 100vh` on the nested layout out-competed the parent's
    // actual, smaller allotted height and pushed content below the fold.
    //
    // `flex-auto` (real antd's `.ant-layout { flex: auto }`) is the other
    // half of that same contract: when a `Layout` is itself a flex child of
    // another `Layout` (the Sider+Content row inside the outer Header+row
    // column, or any nested composition), it must grow to fill whatever
    // space its parent flex container has, not shrink to its own content
    // height. Without it, a `Layout` sizes to `auto` (its content), so a row
    // below a header stops at its shortest child's height instead of
    // filling the viewport — the bug behind the Ontology Builder canvas (and
    // every other nested-Layout page) rendering a few hundred px tall with
    // the rest of the window left blank underneath.
    <div
      ref={ref}
      className={cn(
        "flex w-full flex-auto",
        (hasSider ?? detectedHasSider) ? "flex-row" : "flex-col",
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
});
LayoutInternal.displayName = "Layout";

export const Header = React.forwardRef<HTMLElement, React.ComponentProps<"header">>(
  ({ className, children, ...props }, ref) => (
    <header
      ref={ref}
      className={cn(
        "sticky top-0 z-30 flex h-14 items-center border-b border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-4",
        className,
      )}
      {...props}
    >
      {children}
    </header>
  ),
);
Header.displayName = "Header";

export const Sider = React.forwardRef<
  HTMLElement,
  React.ComponentProps<"aside"> & { readonly width?: number; readonly collapsed?: boolean; readonly collapsedWidth?: number; readonly theme?: "light" | "dark" }
>(({ className, width = 220, collapsed = false, collapsedWidth = 64, theme, style, children, ...props }, ref) => {
  void theme;
  return (
    <aside
      ref={ref}
      className={cn(
        "flex shrink-0 flex-col border-r border-[var(--gowl-border)] bg-[var(--gowl-sider)] transition-[width] duration-200",
        className,
      )}
      style={{ ...style, width: collapsed ? collapsedWidth : width }}
      {...props}
    >
      {children}
    </aside>
  );
});
Sider.displayName = "Sider";

export const Content = React.forwardRef<HTMLElement, React.ComponentProps<"main">>(
  ({ className, children, ...props }, ref) => (
    <main
      ref={ref}
      className={cn(
        "relative flex-1 overflow-auto bg-[var(--gowl-page)] p-4",
        className,
      )}
      {...props}
    >
      {children}
    </main>
  ),
);
Content.displayName = "Content";

export const Footer = React.forwardRef<HTMLElement, React.ComponentProps<"footer">>(
  ({ className, children, ...props }, ref) => (
    <footer
      ref={ref}
      className={cn(
        "flex items-center border-t border-[var(--gowl-border)] bg-[var(--gowl-raised)] px-4 py-2 text-xs text-[var(--gowl-text-muted)]",
        className,
      )}
      {...props}
    >
      {children}
    </footer>
  ),
);
Footer.displayName = "Footer";

export const Layout = Object.assign(LayoutInternal, {
  Header,
  Sider,
  Content,
  Footer,
}) as typeof LayoutInternal & {
  Header: typeof Header;
  Sider: typeof Sider;
  Content: typeof Content;
  Footer: typeof Footer;
};
