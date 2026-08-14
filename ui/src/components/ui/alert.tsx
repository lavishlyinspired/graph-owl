import * as React from "react";
import { cn } from "../../lib/utils";
import {
  CheckCircleOutlined,
  CloseOutlined,
  ExclamationCircleOutlined,
  InfoCircleOutlined,
  WarningOutlined,
} from "./icons";

type AlertVariant = "info" | "success" | "warning" | "error";

interface AlertProps extends Omit<React.ComponentProps<"div">, "title"> {
  readonly message?: React.ReactNode;
  /** Not part of antd's real API; several call sites in this console use it
   *  as a more literal name for the same headline `message` renders. */
  readonly title?: React.ReactNode;
  readonly description?: React.ReactNode;
  readonly type?: AlertVariant;
  readonly showIcon?: boolean;
  readonly icon?: React.ReactNode;
  readonly action?: React.ReactNode;
  readonly closable?: boolean;
  readonly onClose?: () => void;
}

const config: Record<AlertVariant, { icon: React.ReactNode; classes: string }> = {
  info: {
    icon: <InfoCircleOutlined size={16} />,
    classes:
      "border-[var(--gowl-border)] bg-[var(--gowl-fill-subtle)] text-[var(--gowl-text)]",
  },
  success: {
    icon: <CheckCircleOutlined size={16} />,
    classes:
      "border-green-200 bg-green-50 text-green-900",
  },
  warning: {
    icon: <WarningOutlined size={16} />,
    classes:
      "border-amber-200 bg-amber-50 text-amber-900",
  },
  error: {
    icon: <ExclamationCircleOutlined size={16} />,
    classes:
      "border-red-200 bg-red-50 text-red-900",
  },
};

export const Alert = React.forwardRef<HTMLDivElement, AlertProps>(
  (
    {
      className,
      message,
      title,
      description,
      type = "info",
      showIcon = false,
      icon: iconProp,
      action,
      closable = false,
      onClose,
      ...props
    },
    ref,
  ) => {
    const [closed, setClosed] = React.useState(false);
    if (closed) return null;
    const { icon, classes } = config[type];
    return (
      <div
        ref={ref}
        role="alert"
        className={cn(
          "relative flex gap-3 rounded-[var(--gowl-radius-card)] border p-4 text-sm",
          classes,
          className,
        )}
        {...props}
      >
        {showIcon && <span className="mt-0.5 shrink-0">{iconProp ?? icon}</span>}
        <div className="flex-1">
          {message || title ? <div className="font-medium">{message ?? title}</div> : null}
          {description ? (
            <div className="mt-1 opacity-90">{description}</div>
          ) : null}
        </div>
        {action ? <div className="shrink-0">{action}</div> : null}
        {closable ? (
          <button
            type="button"
            aria-label="Close"
            onClick={() => {
              setClosed(true);
              onClose?.();
            }}
            className="ml-2 shrink-0 rounded p-0.5 opacity-60 hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-[var(--gowl-primary)]"
          >
            <CloseOutlined size={12} />
          </button>
        ) : null}
      </div>
    );
  },
);
Alert.displayName = "Alert";

export const AlertError = (props: Omit<AlertProps, "type">) => <Alert type="error" {...props} />;
export const AlertWarning = (props: Omit<AlertProps, "type">) => <Alert type="warning" {...props} />;
export const AlertInfo = (props: Omit<AlertProps, "type">) => <Alert type="info" {...props} />;
export const AlertSuccess = (props: Omit<AlertProps, "type">) => <Alert type="success" {...props} />;
