import * as React from "react";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import { cn } from "../../lib/utils";

export const TabsRoot = TabsPrimitive.Root;

export const TabsList = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.List>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.List>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.List
    ref={ref}
    className={cn(
      "inline-flex h-9 items-center gap-1 rounded-[var(--gowl-radius-control)] bg-[var(--gowl-fill)] p-1",
      className,
    )}
    {...props}
  />
));
TabsList.displayName = TabsPrimitive.List.displayName;

export const TabsTrigger = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Trigger
    ref={ref}
    className={cn(
      "inline-flex items-center justify-center whitespace-nowrap rounded-[var(--gowl-radius-small)] px-3 py-1 text-sm font-medium text-[var(--gowl-text-muted)] transition-colors data-[state=active]:bg-[var(--gowl-raised)] data-[state=active]:text-[var(--gowl-selected-text)] data-[state=active]:shadow-sm",
      className,
    )}
    {...props}
  />
));
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName;

export const TabsContent = React.forwardRef<
  React.ElementRef<typeof TabsPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Content
    ref={ref}
    className={cn("mt-2 focus-visible:outline-none", className)}
    {...props}
  />
));
TabsContent.displayName = TabsPrimitive.Content.displayName;

interface TabItem {
  readonly key: string;
  readonly label: React.ReactNode;
  readonly children?: React.ReactNode;
  readonly disabled?: boolean;
}

interface AntTabsProps extends Omit<React.ComponentPropsWithoutRef<typeof TabsPrimitive.Root>, "value" | "onValueChange" | "onChange"> {
  readonly items?: TabItem[];
  readonly defaultActiveKey?: string;
  readonly activeKey?: string;
  readonly onChange?: (key: string) => void;
  readonly type?: "line" | "card";
  /** Radix unmounts inactive tab content by default (no `forceMount`), so
   *  antd's `destroyOnHidden` is already this component's behavior. */
  readonly destroyOnHidden?: boolean;
}

export const Tabs = React.forwardRef<React.ElementRef<typeof TabsPrimitive.Root>, AntTabsProps>(
  (
    { className, items, defaultActiveKey, activeKey, onChange, type = "line", destroyOnHidden, children, ...props },
    ref,
  ) => {
    void destroyOnHidden;
    if (items) {
      const defaultValue = activeKey ?? defaultActiveKey ?? items[0]?.key;
      return (
        <TabsPrimitive.Root
          ref={ref}
          className={cn("w-full", className)}
          defaultValue={activeKey ? undefined : defaultValue}
          value={activeKey}
          onValueChange={onChange}
          {...props}
        >
          <TabsList
            className={cn(
              type === "card" && "bg-transparent p-0 gap-0 border-b border-[var(--gowl-border)] rounded-none",
            )}
          >
            {items.map((item) => (
              <TabsTrigger
                key={item.key}
                value={item.key}
                disabled={item.disabled}
                className={cn(
                  type === "card" &&
                    "rounded-b-none border border-transparent border-b-[var(--gowl-border)] data-[state=active]:border-[var(--gowl-border)] data-[state=active]:border-b-transparent data-[state=active]:bg-[var(--gowl-raised)] rounded-t-[var(--gowl-radius-control)]",
                )}
              >
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
          {items.map((item) => (
            <TabsContent key={item.key} value={item.key}>
              {item.children}
            </TabsContent>
          ))}
        </TabsPrimitive.Root>
      );
    }
    return (
      <TabsPrimitive.Root
        ref={ref}
        className={cn("w-full", className)}
        defaultValue={defaultActiveKey}
        value={activeKey}
        onValueChange={onChange}
        {...props}
      >
        {children}
      </TabsPrimitive.Root>
    );
  },
);
Tabs.displayName = "Tabs";
