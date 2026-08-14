import * as React from "react";
import { cn } from "../../lib/utils";
import { Label } from "./label";

interface FormRule {
  readonly required?: boolean;
  readonly message?: string;
}

interface FormContextValue {
  readonly values: Record<string, unknown>;
  readonly errors: Record<string, string | undefined>;
  readonly setValue: (name: string, value: unknown) => void;
  readonly registerRules: (name: string, rules: readonly FormRule[]) => void;
}

const FormContext = React.createContext<FormContextValue | null>(null);

interface AntFormProps extends Omit<React.ComponentProps<"form">, "onSubmit"> {
  readonly layout?: "horizontal" | "vertical" | "inline";
  readonly onFinish?: (values: Record<string, unknown>) => void;
  readonly initialValues?: Record<string, unknown>;
}

const FormInternal = React.forwardRef<HTMLFormElement, AntFormProps>(
  ({ className, layout = "vertical", onFinish, initialValues, children, ...props }, ref) => {
    const [values, setValues] = React.useState<Record<string, unknown>>(initialValues ?? {});
    const [errors, setErrors] = React.useState<Record<string, string | undefined>>({});
    const rulesRef = React.useRef<Record<string, readonly FormRule[]>>({});

    const setValue = React.useCallback((name: string, value: unknown) => {
      setValues((prev) => ({ ...prev, [name]: value }));
    }, []);
    const registerRules = React.useCallback((name: string, rules: readonly FormRule[]) => {
      rulesRef.current[name] = rules;
    }, []);

    const context = React.useMemo<FormContextValue>(
      () => ({ values, errors, setValue, registerRules }),
      [values, errors, setValue, registerRules],
    );

    return (
      <FormContext.Provider value={context}>
        <form
          ref={ref}
          className={cn(
            "flex gap-4",
            layout === "inline" ? "flex-row items-end flex-wrap" : "flex-col",
            className,
          )}
          onSubmit={(e) => {
            e.preventDefault();
            const nextErrors: Record<string, string | undefined> = {};
            for (const [name, rules] of Object.entries(rulesRef.current)) {
              for (const rule of rules) {
                const value = values[name];
                if (rule.required && (value === undefined || value === "")) {
                  nextErrors[name] = rule.message ?? "This field is required";
                  break;
                }
              }
            }
            setErrors(nextErrors);
            if (Object.values(nextErrors).every((e) => e === undefined)) onFinish?.(values);
          }}
          {...props}
        >
          {children}
        </form>
      </FormContext.Provider>
    );
  },
);
FormInternal.displayName = "Form";

interface AntFormItemProps {
  readonly label?: React.ReactNode;
  readonly name?: string;
  readonly rules?: readonly FormRule[];
  readonly extra?: React.ReactNode;
  readonly className?: string;
  readonly children: React.ReactElement;
}

function AntFormItem({ label, name, rules, extra, className, children }: AntFormItemProps) {
  const ctx = React.useContext(FormContext);
  React.useEffect(() => {
    if (ctx && name && rules) ctx.registerRules(name, rules);
  }, [ctx, name, rules]);

  const value = name && ctx ? ctx.values[name] : undefined;
  const error = name && ctx ? ctx.errors[name] : undefined;

  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      {label ? (
        <label className="text-sm font-medium text-[var(--gowl-text)]">{label}</label>
      ) : null}
      {name && ctx
        ? React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
            value: value ?? "",
            onChange: (e: React.ChangeEvent<HTMLInputElement> | unknown) => {
              const next =
                e && typeof e === "object" && "target" in e
                  ? (e as React.ChangeEvent<HTMLInputElement>).target.value
                  : e;
              ctx.setValue(name, next);
            },
          })
        : children}
      {extra ? <p className="text-xs text-[var(--gowl-text-subtle)]">{extra}</p> : null}
      {error ? <p className="text-xs font-medium text-red-600">{error}</p> : null}
    </div>
  );
}

export const Form = Object.assign(FormInternal, { Item: AntFormItem }) as typeof FormInternal & {
  Item: typeof AntFormItem;
};

export const FormItem = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, children, ...props }, ref) => (
    <div ref={ref} className={cn("flex flex-col gap-1.5", className)} {...props}>
      {children}
    </div>
  ),
);
FormItem.displayName = "FormItem";

export const FormLabel = React.forwardRef<
  React.ElementRef<typeof Label>,
  React.ComponentPropsWithoutRef<typeof Label>
>(({ className, children, ...props }, ref) => (
  <Label ref={ref} className={cn(className)} {...props}>
    {children}
  </Label>
));
FormLabel.displayName = "FormLabel";

export const FormControl = React.forwardRef<HTMLDivElement, React.ComponentProps<"div">>(
  ({ className, children, ...props }, ref) => (
    <div ref={ref} className={cn("", className)} {...props}>
      {children}
    </div>
  ),
);
FormControl.displayName = "FormControl";

export const FormDescription = React.forwardRef<HTMLParagraphElement, React.ComponentProps<"p">>(
  ({ className, children, ...props }, ref) => (
    <p
      ref={ref}
      className={cn("text-xs text-[var(--gowl-text-subtle)]", className)}
      {...props}
    >
      {children}
    </p>
  ),
);
FormDescription.displayName = "FormDescription";

export const FormMessage = React.forwardRef<HTMLParagraphElement, React.ComponentProps<"p">>(
  ({ className, children, ...props }, ref) =>
    children ? (
      <p
        ref={ref}
        className={cn("text-xs font-medium text-red-600", className)}
        {...props}
      >
        {children}
      </p>
    ) : null,
);
FormMessage.displayName = "FormMessage";
