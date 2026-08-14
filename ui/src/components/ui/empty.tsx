import * as React from "react";
import { cn } from "../../lib/utils";
import { SearchOutlined } from "./icons";

interface EmptyProps extends React.ComponentProps<"div"> {
  readonly description?: React.ReactNode;
  readonly image?: React.ReactNode;
}

const EmptyInternal = React.forwardRef<HTMLDivElement, EmptyProps>(
  ({ className, description = "No data", image, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        "flex flex-col items-center justify-center gap-2 py-12 text-center text-[var(--gowl-text-muted)]",
        className,
      )}
      {...props}
    >
      {image ?? <SearchOutlined size={48} className="opacity-30" />}
      <p className="text-sm">{description}</p>
      {children}
    </div>
  ),
);
EmptyInternal.displayName = "Empty";

export const Empty = Object.assign(EmptyInternal, { PRESENTED_IMAGE_SIMPLE: "simple" }) as typeof EmptyInternal & {
  PRESENTED_IMAGE_SIMPLE: "simple";
};
