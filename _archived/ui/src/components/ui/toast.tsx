import * as React from "react";
import { cn } from "../../lib/utils";
import {
  CheckCircleFilled,
  CloseCircleFilled,
  CloseOutlined,
  InfoCircleOutlined,
  WarningOutlined,
} from "./icons";

type ToastType = "success" | "error" | "info" | "warning" | "loading";

interface Toast {
  readonly id: string;
  readonly type: ToastType;
  readonly content: React.ReactNode;
  readonly duration?: number;
}

interface ToastContextValue {
  readonly open: (toast: Omit<Toast, "id">) => string;
  readonly close: (id: string) => void;
  readonly success: (content: React.ReactNode, duration?: number) => string;
  readonly error: (content: React.ReactNode, duration?: number) => string;
  readonly info: (content: React.ReactNode, duration?: number) => string;
  readonly warning: (content: React.ReactNode, duration?: number) => string;
  readonly loading: (content: React.ReactNode, duration?: number) => string;
  readonly destroy: () => void;
}

const ToastContext = React.createContext<ToastContextValue | null>(null);

/** Global imperative handle so `message.success(...)` works outside React
 *  render phases (e.g. inside event handlers in deeply nested modules). The
 *  provider writes its real implementation here on mount; before mount the
 *  methods are no-ops so a very early call is silently dropped rather than
 *  throwing. */
const globalToast: {
  current: Pick<ToastContextValue, "success" | "error" | "info" | "warning" | "loading" | "destroy">;
} = {
  current: {
    success: () => "",
    error: () => "",
    info: () => "",
    warning: () => "",
    loading: () => "",
    destroy: () => undefined,
  },
};

export function useToast(): ToastContextValue {
  const ctx = React.useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used within ToastProvider");
  return ctx;
}

const config: Record<ToastType, { icon: React.ReactNode; classes: string }> = {
  success: {
    icon: <CheckCircleFilled size={18} />,
    classes: "border-green-200 bg-green-50 text-green-900",
  },
  error: {
    icon: <CloseCircleFilled size={18} />,
    classes: "border-red-200 bg-red-50 text-red-900",
  },
  info: {
    icon: <InfoCircleOutlined size={18} />,
    classes: "border-[var(--gowl-border)] bg-[var(--gowl-raised)] text-[var(--gowl-text)]",
  },
  warning: {
    icon: <WarningOutlined size={18} />,
    classes: "border-amber-200 bg-amber-50 text-amber-900",
  },
  loading: {
    icon: (
      <svg
        width={18}
        height={18}
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
    ),
    classes: "border-[var(--gowl-border)] bg-[var(--gowl-raised)] text-[var(--gowl-text)]",
  },
};

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = React.useState<Toast[]>([]);
  const timers = React.useRef<Record<string, number>>({});

  const close = React.useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
    window.clearTimeout(timers.current[id]);
    delete timers.current[id];
  }, []);

  const open = React.useCallback(
    (toast: Omit<Toast, "id">) => {
      const id = `${Date.now()}-${Math.random()}`;
      const duration = toast.duration ?? (toast.type === "error" ? 5000 : 3000);
      setToasts((prev) => [...prev, { ...toast, id, duration }]);
      if (duration > 0) {
        timers.current[id] = window.setTimeout(() => close(id), duration);
      }
      return id;
    },
    [close],
  );

  const success = React.useCallback(
    (content: React.ReactNode, duration?: number) =>
      open({ type: "success", content, duration }),
    [open],
  );
  const error = React.useCallback(
    (content: React.ReactNode, duration?: number) =>
      open({ type: "error", content, duration }),
    [open],
  );
  const info = React.useCallback(
    (content: React.ReactNode, duration?: number) => open({ type: "info", content, duration }),
    [open],
  );
  const warning = React.useCallback(
    (content: React.ReactNode, duration?: number) =>
      open({ type: "warning", content, duration }),
    [open],
  );
  const loading = React.useCallback(
    (content: React.ReactNode, duration?: number) =>
      open({ type: "loading", content, duration: duration ?? 0 }),
    [open],
  );
  const destroy = React.useCallback(() => {
    setToasts([]);
    Object.values(timers.current).forEach((t) => window.clearTimeout(t));
    timers.current = {};
  }, []);

  const value = React.useMemo(
    () => ({ open, close, success, error, info, warning, loading, destroy }),
    [open, close, success, error, info, warning, loading, destroy],
  );

  React.useEffect(() => {
    globalToast.current = { success, error, info, warning, loading, destroy };
    return () => {
      globalToast.current = {
        success: () => "",
        error: () => "",
        info: () => "",
        warning: () => "",
        loading: () => "",
        destroy: () => undefined,
      };
    };
  }, [success, error, info, warning, loading, destroy]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div
        aria-live="polite"
        aria-atomic="true"
        className="pointer-events-none fixed right-4 top-4 z-[100] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2"
      >
        {toasts.map((toast) => {
          const { icon, classes } = config[toast.type];
          return (
            <div
              key={toast.id}
              className={cn(
                "pointer-events-auto flex items-start gap-3 rounded-[var(--gowl-radius-card)] border p-3 text-sm shadow-[var(--gowl-shadow-medium)]",
                classes,
              )}
            >
              <span className="mt-0.5 shrink-0">{icon}</span>
              <span className="flex-1">{toast.content}</span>
              <button
                type="button"
                onClick={() => close(toast.id)}
                className="ml-1 shrink-0 opacity-60 hover:opacity-100"
                aria-label="Close"
              >
                <CloseOutlined size={12} />
              </button>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

/** Ant Design `message` compatibility: imperative toast API. */
export const message = {
  success: (content: React.ReactNode, duration?: number) =>
    globalToast.current.success(content, duration),
  error: (content: React.ReactNode, duration?: number) =>
    globalToast.current.error(content, duration),
  info: (content: React.ReactNode, duration?: number) =>
    globalToast.current.info(content, duration),
  warning: (content: React.ReactNode, duration?: number) =>
    globalToast.current.warning(content, duration),
  loading: (content: React.ReactNode, duration?: number) =>
    globalToast.current.loading(content, duration),
  destroy: () => globalToast.current.destroy(),
};
